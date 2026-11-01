use amarcode_daemon::{
    protocol::rpc::{PromptParams, UpdatePromptQueueParams},
    service::{prompt_queue::PromptQueue, ChatManager, PromptResult},
    store::Store,
    Error,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
fn setup() -> (
    Arc<Store>,
    String,
    broadcast::Sender<amarcode_daemon::protocol::EditorEvent>,
) {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let (events, _) = broadcast::channel(64);
    let chat = ChatManager::new(store.clone(), events.clone())
        .create("/workspace", None)
        .unwrap();
    (store, chat.id, events)
}
fn prompt(chat: &str, text: &str) -> PromptParams {
    PromptParams {
        chat_id: chat.into(),
        agent_id: "agent".into(),
        text: text.into(),
        attachments: vec![],
        config_values: vec![],
    }
}
fn result(p: PromptParams) -> PromptResult {
    PromptResult {
        stop_reason: Some("end_turn".into()),
        run_id: "run".into(),
        chat_id: p.chat_id,
        agent_id: p.agent_id,
        user_message_id: p.text,
        acp_session_id: Some("session".into()),
    }
}
fn action(
    chat: &str,
    action: &str,
    id: Option<&str>,
    text: Option<&str>,
) -> UpdatePromptQueueParams {
    UpdatePromptQueueParams {
        chat_id: chat.into(),
        action: action.into(),
        id: id.map(str::to_owned),
        text: text.map(str::to_owned),
    }
}
fn wait(mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(Instant::now() < end, "queue did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn fifo_dedup_edit_remove_and_pause_resume() {
    let (store, chat, events) = setup();
    let (started, read) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let receive = Mutex::new(receive);
    let queue = PromptQueue::new(store, events, move |p, _| {
        started.send(p.text.clone()).unwrap();
        receive.lock().unwrap().recv().unwrap();
        Ok(result(p))
    });
    queue.enqueue("one", prompt(&chat, "first")).unwrap();
    assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap(), "first");
    queue.enqueue("two", prompt(&chat, "second")).unwrap();
    queue.enqueue("two", prompt(&chat, "second")).unwrap();
    queue.enqueue("three", prompt(&chat, "remove")).unwrap();
    assert_eq!(queue.state(&chat).unwrap().items.len(), 3);
    assert!(queue.enqueue("two", prompt(&chat, "different")).is_err());
    assert!(queue
        .update(action(&chat, "edit", Some("one"), Some("already running")))
        .is_err());
    queue
        .update(action(&chat, "edit", Some("two"), Some("edited")))
        .unwrap();
    queue
        .update(action(&chat, "remove", Some("three"), None))
        .unwrap();
    queue.update(action(&chat, "pause", None, None)).unwrap();
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().items.len() == 1);
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
    queue.update(action(&chat, "resume", None, None)).unwrap();
    assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap(), "edited");
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().items.is_empty());
    queue.enqueue("one", prompt(&chat, "first")).unwrap();
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
}
#[test]
fn failure_pauses_and_resume_does_not_replay_failed_work() {
    let (store, chat, events) = setup();
    let (started, read) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let receive = Mutex::new(receive);
    let queue = PromptQueue::new(store, events, move |p, _| {
        started.send(p.text.clone()).unwrap();
        receive.lock().unwrap().recv().unwrap();
        if p.text == "fail" {
            Err(Error::msg("failed"))
        } else {
            Ok(result(p))
        }
    });
    queue.enqueue("bad", prompt(&chat, "fail")).unwrap();
    read.recv_timeout(Duration::from_secs(2)).unwrap();
    queue.enqueue("next", prompt(&chat, "next")).unwrap();
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().paused);
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
    assert_eq!(queue.state(&chat).unwrap().items[0].status, "failed");
    queue.update(action(&chat, "resume", None, None)).unwrap();
    assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap(), "next");
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().items.len() == 1);
    assert_eq!(queue.state(&chat).unwrap().items[0].id, "bad");
}
#[test]
fn stop_marks_a_starting_delivery_and_holds_followups() {
    let (store, chat, events) = setup();
    let (started, read) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let receive = Mutex::new(receive);
    let saw_stop = Arc::new(AtomicBool::new(false));
    let check = saw_stop.clone();
    let queue = PromptQueue::new(store, events, move |p, stopped| {
        started.send(()).unwrap();
        receive.lock().unwrap().recv().unwrap();
        check.store(stopped.load(Ordering::SeqCst), Ordering::SeqCst);
        Ok(result(p))
    });
    queue.enqueue("first", prompt(&chat, "first")).unwrap();
    read.recv_timeout(Duration::from_secs(2)).unwrap();
    queue.enqueue("next", prompt(&chat, "next")).unwrap();
    queue.pause(&chat, "Stopped by you").unwrap();
    release.send(()).unwrap();
    wait(|| saw_stop.load(Ordering::SeqCst));
    wait(|| queue.state(&chat).unwrap().items[0].status == "failed");
    assert!(queue.state(&chat).unwrap().paused);
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
}
#[test]
fn restart_preserves_waiting_jobs_but_never_replays_a_running_job() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("queue.sqlite");
    let chat;
    {
        let store = Arc::new(Store::open(&db).unwrap());
        let (events, _) = broadcast::channel(4);
        chat = ChatManager::new(store.clone(), events)
            .create("/workspace", None)
            .unwrap()
            .id;
        store
            .insert_prompt_job("interrupted", &prompt(&chat, "old running job"))
            .unwrap();
        store
            .insert_prompt_job("waiting", &prompt(&chat, "waiting job"))
            .unwrap();
        store.claim_prompt_job(&chat).unwrap();
    }
    let store = Arc::new(Store::open(&db).unwrap());
    store.recover_prompt_queues().unwrap();
    assert_eq!(store.delete_empty_chats().unwrap(), 0);
    let state = store.prompt_queue(&chat).unwrap();
    assert!(state.paused);
    assert_eq!(state.items[0].status, "interrupted");
    assert_eq!(state.items[1].status, "queued");
    let (events, _) = broadcast::channel(4);
    let (send, read) = mpsc::channel();
    let queue = PromptQueue::new(store, events, move |p, _| {
        send.send(p.text.clone()).unwrap();
        Ok(result(p))
    });
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
    queue.update(action(&chat, "resume", None, None)).unwrap();
    assert_eq!(
        read.recv_timeout(Duration::from_secs(2)).unwrap(),
        "waiting job"
    );
    wait(|| queue.state(&chat).unwrap().items.len() == 1);
}
#[test]
fn legacy_prompt_rpc_and_followups_share_one_fifo() {
    let (store, chat, events) = setup();
    let (send, read) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let receive = Mutex::new(receive);
    let queue = PromptQueue::new(store, events, move |p, _| {
        send.send(p.text.clone()).unwrap();
        receive.lock().unwrap().recv().unwrap();
        Ok(result(p))
    });
    let direct = queue.clone();
    let first = prompt(&chat, "direct");
    let thread = std::thread::spawn(move || direct.prompt(first));
    assert_eq!(read.recv_timeout(Duration::from_secs(2)).unwrap(), "direct");
    queue
        .enqueue("followup", prompt(&chat, "followup"))
        .unwrap();
    release.send(()).unwrap();
    assert_eq!(thread.join().unwrap().unwrap().user_message_id, "direct");
    assert_eq!(
        read.recv_timeout(Duration::from_secs(2)).unwrap(),
        "followup"
    );
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().items.is_empty());
}

#[test]
fn attachment_config_and_queue_limits_are_preserved() {
    let (store, chat, events) = setup();
    let queue = PromptQueue::new(store.clone(), events, |_, _| {
        panic!("paused queue must not execute")
    });
    queue.update(action(&chat, "pause", None, None)).unwrap();
    let mut first = prompt(&chat, "original");
    first.attachments = serde_json::from_value(
        serde_json::json!([{"mime_type":"text/plain","data":"dGVzdA==","filename":"notes.txt"}]),
    )
    .unwrap();
    first.config_values = serde_json::from_value(
        serde_json::json!([{"config_id":"mode","value":{"type":"boolean","value":true}}]),
    )
    .unwrap();
    queue.enqueue("attached", first.clone()).unwrap();
    queue
        .update(action(&chat, "edit", Some("attached"), Some("edited text")))
        .unwrap();
    let (_, restored) = store.claim_prompt_job(&chat).unwrap().unwrap();
    assert_eq!(restored.text, "edited text");
    assert_eq!(
        serde_json::to_value(restored.attachments).unwrap(),
        serde_json::to_value(first.attachments).unwrap()
    );
    assert_eq!(
        serde_json::to_value(restored.config_values).unwrap(),
        serde_json::to_value(first.config_values).unwrap()
    );
    assert!(queue
        .update(action(&chat, "remove", Some("attached"), None))
        .is_err());
    for i in 0..15 {
        queue
            .enqueue(&format!("pending-{i}"), prompt(&chat, "waiting"))
            .unwrap();
    }
    assert!(queue
        .enqueue("overflow", prompt(&chat, "too many"))
        .is_err());
    assert_eq!(queue.state(&chat).unwrap().items.len(), 16);
    assert!(queue.enqueue("empty", prompt(&chat, "  ")).is_err());
}
#[test]
fn concurrent_retries_only_deliver_once() {
    let (store, chat, events) = setup();
    let (send, read) = mpsc::channel();
    let (release, receive) = mpsc::channel();
    let receive = Mutex::new(receive);
    let queue = PromptQueue::new(store, events, move |p, _| {
        send.send(p.text.clone()).unwrap();
        receive.lock().unwrap().recv().unwrap();
        Ok(result(p))
    });
    queue
        .enqueue("blocking", prompt(&chat, "blocking"))
        .unwrap();
    read.recv_timeout(Duration::from_secs(2)).unwrap();
    let threads = (0..8)
        .map(|_| {
            let queue = queue.clone();
            let chat = chat.clone();
            std::thread::spawn(move || {
                queue
                    .enqueue("retry-id", prompt(&chat, "followup"))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(queue.state(&chat).unwrap().items.len(), 2);
    release.send(()).unwrap();
    assert_eq!(
        read.recv_timeout(Duration::from_secs(2)).unwrap(),
        "followup"
    );
    release.send(()).unwrap();
    wait(|| queue.state(&chat).unwrap().items.is_empty());
    assert!(read.recv_timeout(Duration::from_millis(80)).is_err());
}
