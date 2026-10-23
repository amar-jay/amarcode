---
title: Attachments and dictation
description: Add images, pasted text, and spoken input to your prompts.
---

## Attach an image or text

Paste supported content into the composer and review its attachment preview before sending. A prompt supports up to four attachments.

![An image attached in a conversation, shown above the agent’s reply.](/images/workspace/attachment.webp)

| Content | Limit |
| --- | --- |
| PNG, JPEG, WebP, or GIF image | 10 MB per image |
| UTF-8 plain text | 1 MB per attachment |

The daemon checks an image's content against its declared media type. Image prompts also require the agent to advertise image support. An agent without that capability rejects the image prompt.

Attachments are uploaded to and stored on the selected daemon host. Plain-text attachments are sent to the agent as text; images are sent as image blocks.

:::note[Restoring older conversations]
Attachments remain available to Amarcode's UI, but historical attachments are not replayed by the daemon's fallback history restoration. Attach a file again if a new agent needs its contents.
:::

## Dictate a prompt

Use the microphone control in the composer and grant microphone access if requested. Record your speech, stop recording, and review the transcribed text before sending it.

![The dictation controls in the composer, showing a recording timer and discard option.](/images/workspace/dictation.webp)

Dictation requires a provider configured in **Settings → ACP provider**. That provider must support `/audio/transcriptions` and the requested model, `openai/whisper-large-v3-turbo`.

The audio upload limit is 25 MB. The daemon forwards audio in memory to the provider and does not save it to disk. Your provider's own data-handling policies still apply.

## If dictation fails

Check microphone access, provider credentials, endpoint support, and the recording size. A working chat model does not by itself establish transcription support. Try a shorter recording if the upload exceeds the limit.
