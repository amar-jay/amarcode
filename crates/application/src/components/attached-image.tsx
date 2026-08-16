import { daemonApi } from "@/api";
import { MessageDetail, MessagePart } from "@/types";
import { FileTextIcon, LoaderCircle } from "lucide-react";
import { useState, useEffect } from "react";

export type StoredImagePart = {
  attachmentId: string;
  filename?: string | null;
  mediaType: string;
};

function StoredImage({
  chatId,
  image,
}: {
  chatId: string;
  image: StoredImagePart;
}) {
  const [source, setSource] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    setSource(null);
    setFailed(false);
    void daemonApi
      .getAttachment(chatId, image.attachmentId)
      .then((result) => {
        if (active)
          setSource(`data:${result.media_type};base64,${result.data}`);
      })
      .catch(() => {
        if (active) setFailed(true);
      });
    return () => {
      active = false;
    };
  }, [chatId, image.attachmentId]);

  if (failed) {
    return (
      <div className="flex h-28 w-40 items-center justify-center rounded-lg border text-xs text-muted-foreground">
        Image unavailable
      </div>
    );
  }
  return (
    <div className="h-40 max-w-64 overflow-hidden rounded-lg border bg-muted">
      {source ? (
        <img
          alt={image.filename || "Pasted image"}
          className="size-full object-contain"
          src={source}
        />
      ) : (
        <div className="flex size-full items-center justify-center">
          <LoaderCircle className="size-4 animate-spin text-muted-foreground" />
        </div>
      )}
    </div>
  );
}

function parseStoredImage(part: MessagePart): StoredImagePart | null {
  if (part.kind !== "image") return null;
  try {
    const value = JSON.parse(part.content_json) as Partial<StoredImagePart>;
    return typeof value.attachmentId === "string" &&
      typeof value.mediaType === "string"
      ? {
          attachmentId: value.attachmentId,
          filename: typeof value.filename === "string" ? value.filename : null,
          mediaType: value.mediaType,
        }
      : null;
  } catch {
    return null;
  }
}

export function AttachedImages({ item }: { item: MessageDetail }) {
  const images = item.parts
    .map(parseStoredImage)
    .filter((image): image is StoredImagePart => image !== null);
  if (images.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {images.map((image) => (
        <StoredImage
          chatId={item.message.chat_id}
          image={image}
          key={image.attachmentId}
        />
      ))}
    </div>
  );
}

type StoredFilePart = {
  attachmentId: string;
  filename?: string | null;
  mediaType: string;
};

function parseStoredFile(part: MessagePart): StoredFilePart | null {
  if (part.kind !== "file") return null;
  try {
    const value = JSON.parse(part.content_json) as Partial<StoredFilePart>;
    return typeof value.attachmentId === "string" &&
      typeof value.mediaType === "string"
      ? {
          attachmentId: value.attachmentId,
          filename: typeof value.filename === "string" ? value.filename : null,
          mediaType: value.mediaType,
        }
      : null;
  } catch {
    return null;
  }
}

export function AttachedFiles({ item }: { item: MessageDetail }) {
  const files = item.parts
    .map(parseStoredFile)
    .filter((file): file is StoredFilePart => file !== null);
  if (files.length === 0) return null;

  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {files.map((file) => (
        <div
          className="flex items-center gap-1.5 rounded-md border px-2 py-1 text-sm"
          key={file.attachmentId}
        >
          <FileTextIcon className="size-4 text-muted-foreground" />
          <span>{file.filename || "Text attachment"}</span>
        </div>
      ))}
    </div>
  );
}
