import { ArrowUpRight } from "lucide-react";

export default function RemoteWorkspace() {
  return (
    <section
      id="remote"
      aria-labelledby="remote-title"
      className="grid grid-cols-[1fr_1.25fr] border-t border-rule bg-panel max-[901px]:grid-cols-1"
    >
      <div className="px-[54px] py-[65px] max-[901px]:px-7 max-[901px]:py-10 max-[601px]:px-5 max-[601px]:py-9">
        <p className="mb-[22px] text-[16px] tracking-[0.065em] uppercase max-[601px]:text-[13px]">
          Your workspace, remotely
        </p>
        <h2
          id="remote-title"
          className="mb-6 text-[clamp(38px,4.4vw,60px)] leading-[1.07] font-light tracking-[-0.035em]"
        >
          Work where
          <br />
          your code lives.
        </h2>
        <p className="mb-5 text-[22px] leading-[1.4] text-muted-ink max-[601px]:text-[20px]">
          Run the daemon on another machine and connect from your desktop.
          Project files, Git commands, and coding agents run on that host.
        </p>
        <p className="mb-8 text-[20px] leading-[1.45] text-muted-ink">
          Connect with the host’s WebSocket URL and access token. Your
          conversations stay with that daemon, and active runs continue when you
          close the desktop.
        </p>
        <a
          href="/docs/remote/setup/"
          className="inline-flex items-center gap-3 text-[19px] text-ink no-underline hover:underline"
        >
          Set up a remote workspace{" "}
          <ArrowUpRight size={17} aria-hidden="true" />
        </a>
      </div>
      <figure className="m-0 flex flex-col justify-center border-l border-rule px-7 py-10 max-[901px]:border-t max-[901px]:border-l-0 max-[901px]:pt-7 max-[601px]:px-4 max-[601px]:py-6">
        <a
          href="/images/workspace/remote-connection.webp"
          target="_blank"
          rel="noreferrer"
          aria-label="Open full-size remote connection settings screenshot"
        >
          <img
            src="/images/workspace/remote-connection.webp"
            width={803}
            height={502}
            loading="lazy"
            decoding="async"
            alt="Amarcode connection settings with remote daemon enabled, WebSocket URL and access token fields, and an Apply and reload button."
            className="block h-auto w-full border border-rule"
          />
        </a>
        <figcaption className="mt-4 text-[16px] leading-[1.4] text-muted-ink">
          Your desktop here. Your files and agents on the connected host.
        </figcaption>
      </figure>
    </section>
  );
}
