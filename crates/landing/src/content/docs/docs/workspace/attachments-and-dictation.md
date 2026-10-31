---
title: Attachments and dictation
description: Add images, pasted text, and spoken input to your prompts.
---

## Attach an image or text

Paste an image or text into the composer and review its attachment preview before sending. A prompt supports up to four attachments.

![An image attached in a conversation, shown above the agent’s reply.](/images/workspace/attachment.webp)

| Content | Limit |
| --- | --- |
| PNG, JPEG, WebP, or GIF image | 10 MB per image |
| UTF-8 plain text | 1 MB per attachment |

Images require an agent with image support. Attachments are saved on the computer running your daemon.

When switching agents or reopening an older conversation, reattach files the agent needs to inspect.

## Dictate a prompt

Choose the microphone control and allow microphone access when prompted. Speak, stop recording, and review the text before sending.

![The dictation controls in the composer, showing a recording timer and discard option.](/images/workspace/dictation.webp)

Dictation needs an internet connection, but no API key or provider setup. Your chat provider settings do not affect it. Recordings can be up to two minutes and 25 MB.

Audio is sent to Amarcode's transcription service for processing. The daemon does not save the recording.

## If dictation fails

Check microphone permissions and your internet connection. Retry the recording, or use a shorter clip if it exceeds the upload limit.
