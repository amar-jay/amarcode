use amarcode_daemon::{
    protocol::{Message, MessageRole, MessageStatus},
    service::ChatManager,
    store::Store,
};
use std::{path::Path, sync::Arc};
use tokio::sync::broadcast;

#[test]
fn pages_are_bounded_stable_and_leave_full_agent_history_intact() {
    let store = Arc::new(Store::open(Path::new(":memory:")).unwrap());
    let (events, _) = broadcast::channel(4);
    let chats = ChatManager::new(store.clone(), events);
    let chat = chats.create("/workspace", None).unwrap();
    let add = |id: &str| {
        store
            .create_message(&Message {
                id: id.into(),
                chat_id: chat.id.clone(),
                agent_run_id: None,
                role: MessageRole::User,
                content: format!("original {id}"),
                status: MessageStatus::Complete,
                created_at: "2026-11-01T00:00:00Z".into(),
                updated_at: "2026-11-01T00:00:00Z".into(),
            })
            .unwrap();
    };
    // Identical timestamps exercise the ID tie-breaker at page boundaries.
    for i in 0..120 {
        add(&format!("message-{i:03}"));
    }
    let first = chats.get_with_message_page(&chat.id, 40, None).unwrap();
    assert_eq!(first.messages.len(), 40);
    assert_eq!(first.messages[0].message.id, "message-080");
    assert_eq!(first.messages.last().unwrap().message.id, "message-119");
    assert!(first.has_older_messages);
    add("message-120");
    let second = chats
        .get_with_message_page(&chat.id, 40, Some("message-080"))
        .unwrap();
    assert_eq!(second.messages.len(), 40);
    assert_eq!(second.messages[0].message.id, "message-040");
    assert_eq!(second.messages.last().unwrap().message.id, "message-079");
    assert!(second.has_older_messages);
    let third = chats
        .get_with_message_page(&chat.id, 40, Some("message-040"))
        .unwrap();
    assert_eq!(third.messages.len(), 40);
    assert_eq!(third.messages[0].message.content, "original message-000");
    assert!(!third.has_older_messages);
    let other = chats.create("/elsewhere", None).unwrap();
    assert!(store
        .message_page(&other.id, 40, Some("message-080"))
        .is_err());
    assert!(store.message_page(&chat.id, 40, Some("missing")).is_err());
    assert!(store.message_page(&chat.id, 0, None).is_err());
    assert!(store.message_page(&chat.id, 201, None).is_err());
    assert_eq!(
        chats.get_with_messages(&chat.id).unwrap().messages.len(),
        121
    );
    let empty = chats.get_with_message_page(&other.id, 40, None).unwrap();
    assert!(empty.messages.is_empty());
    assert!(!empty.has_older_messages);
}
