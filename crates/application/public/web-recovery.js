// Runs before module imports, so even a stale entry's missing chunks can recover.
(() => {
  let shown = false;
  const recover = (event) => {
    const message = String(
      event.message || event.reason?.message || event.payload?.message || "",
    );
    const entryFailed =
      event.target?.tagName === "SCRIPT" && event.target.type === "module";
    if (
      shown ||
      (!entryFailed &&
        !/dynamically imported module|loading dynamically|failed to fetch.*module|importing a module script|loading chunk/i.test(
          message,
        ))
    )
      return;
    shown = true;
    const root = document.getElementById("root");
    if (!root) return;
    const panel = document.createElement("main");
    panel.style.cssText =
      "max-width:420px;margin:15vh auto;padding:24px;font:16px system-ui;line-height:1.5";
    panel.setAttribute("role", "alert");
    const heading = document.createElement("h1");
    heading.textContent = "Reload Amarcode";
    const text = document.createElement("p");
    text.textContent =
      "The application was updated or a required file could not load. Reload to get the current version. You will need to reconnect to your daemon.";
    const button = document.createElement("button");
    button.textContent = "Reload application";
    button.style.cssText = "padding:10px 16px;cursor:pointer";
    button.addEventListener("click", () => window.location.reload());
    panel.append(heading, text, button);
    root.hidden = true;
    document.body.append(panel);
  };
  window.addEventListener("error", recover, true);
  window.addEventListener("unhandledrejection", recover);
  window.addEventListener("vite:preloadError", recover);
})();
