import { daemonApi } from "@/api";
import { MessageDetail, MessagePart } from "@/types";
import { FileTextIcon, LoaderCircle } from "lucide-react";
import { useState, useEffect } from "react";
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from "./ui/dialog";

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
  const [expanded, setExpanded] = useState(false);

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
        <Dialog open={expanded} onOpenChange={setExpanded}>
          <DialogTrigger asChild>
            <button
              type="button"
              className="size-full cursor-zoom-in rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              aria-label={`Expand ${image.filename || "attached image"}`}
            >
              <img
                alt={image.filename || "Pasted image"}
                className="size-full object-contain"
                src={source}
              />
            </button>
          </DialogTrigger>
          <DialogContent
            className="top-[calc(50%+1.125rem)] flex h-[85dvh] w-[90vw] max-w-none items-center justify-center overflow-hidden bg-transparent p-6 shadow-none ring-0 sm:max-w-none [&_[data-slot=dialog-close]]:top-3 [&_[data-slot=dialog-close]]:right-3 [&_[data-slot=dialog-close]]:bg-black/50 [&_[data-slot=dialog-close]]:text-white"
            onPointerDown={(event) => {
              const target = event.target;
              if (!(target instanceof HTMLImageElement)) {
                if (target === event.currentTarget) setExpanded(false);
                return;
              }

              const bounds = target.getBoundingClientRect();
              const imageRatio = target.naturalWidth / target.naturalHeight;
              const boxRatio = bounds.width / bounds.height;
              const renderedWidth =
                imageRatio > boxRatio
                  ? bounds.width
                  : bounds.height * imageRatio;
              const renderedHeight =
                imageRatio > boxRatio
                  ? bounds.width / imageRatio
                  : bounds.height;
              const left = bounds.left + (bounds.width - renderedWidth) / 2;
              const top = bounds.top + (bounds.height - renderedHeight) / 2;

              if (
                event.clientX < left ||
                event.clientX > left + renderedWidth ||
                event.clientY < top ||
                event.clientY > top + renderedHeight
              ) {
                setExpanded(false);
              }
            }}
          >
            <DialogTitle className="sr-only">
              {image.filename || "Attached image"}
            </DialogTitle>
            <img
              alt={image.filename || "Pasted image"}
              className="size-full object-contain"
              src={source}
            />
          </DialogContent>
        </Dialog>
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
