import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { LogIn } from "lucide-react";
import { isValidElement } from "react";

const mocks = vi.hoisted(() => ({
  focused: vi.fn(),
  permission: vi.fn(),
  request: vi.fn(),
  send: vi.fn(),
  pageFocused: vi.fn(),
  toast: Object.assign(vi.fn(), {
    info: vi.fn(),
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
    dismiss: vi.fn(),
  }),
}));
vi.mock("@/lib/platform", () => ({ isWeb: false }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ isFocused: mocks.focused }),
}));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: mocks.permission,
  requestPermission: mocks.request,
  sendNotification: mocks.send,
}));
vi.mock("sonner", () => ({ toast: mocks.toast }));
import { notify, notifyAttention, notifyToast } from "./notify";

beforeEach(() => {
  vi.clearAllMocks();
  mocks.focused.mockResolvedValue(false);
  mocks.permission.mockResolvedValue(true);
  mocks.pageFocused.mockReturnValue(false);
  vi.stubGlobal("document", { hasFocus: mocks.pageFocused });
});
afterEach(() => vi.unstubAllGlobals());
const settle = async () => {
  for (let i = 0; i < 12; i++) await Promise.resolve();
};

test("shows a toast but no native notification when the native window is focused", async () => {
  mocks.focused.mockResolvedValue(true);
  notify("Hello");
  await settle();
  expect(mocks.toast.info).toHaveBeenCalledWith("Hello", undefined);
  expect(mocks.send).not.toHaveBeenCalled();
  expect(mocks.permission).not.toHaveBeenCalled();
});

test("suppresses native notifications when the page is focused even if native focus is stale", async () => {
  mocks.pageFocused.mockReturnValue(true);
  notifyToast("Hello");
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
  expect(mocks.permission).not.toHaveBeenCalled();
});

test("rechecks native focus after a delayed permission check", async () => {
  let finish!: (permission: boolean) => void;
  mocks.permission.mockReturnValue(
    new Promise<boolean>((resolve) => {
      finish = resolve;
    }),
  );
  notify("Hello");
  await settle();
  expect(mocks.permission).toHaveBeenCalledOnce();
  mocks.focused.mockResolvedValue(true);
  finish(true);
  await settle();
  expect(mocks.focused).toHaveBeenCalledTimes(2);
  expect(mocks.send).not.toHaveBeenCalled();
});

test("checks page focus after the final asynchronous native check", async () => {
  mocks.focused
    .mockResolvedValueOnce(false)
    .mockImplementationOnce(async () => {
      mocks.pageFocused.mockReturnValue(true);
      return false;
    });
  notify("Hello");
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
});

test("still sends native notifications while unfocused", async () => {
  notifyToast("Hello", { description: "Details" });
  await settle();
  expect(mocks.send).toHaveBeenCalledWith({
    title: "Amarcode",
    body: "Details",
  });
});

test("icon actions keep their accessible label and dismiss the toast after activation", async () => {
  mocks.toast.mockReturnValue("icon-toast");
  const onClick = vi.fn();
  notifyToast("Sign in required", {
    action: { label: "Sign in", icon: LogIn, onClick },
  });
  const options = mocks.toast.mock.calls[0][1];
  const action = options.action;
  if (
    !isValidElement<{ label: string; icon: typeof LogIn; onClick: () => void }>(
      action,
    )
  ) {
    throw new Error("Expected an icon action element");
  }
  expect(action.props.label).toBe("Sign in");
  expect(action.props.icon).toBe(LogIn);
  action.props.onClick();
  expect(onClick).toHaveBeenCalledOnce();
  expect(mocks.toast.dismiss).toHaveBeenCalledWith("icon-toast");
  await settle();
});

test("approval and input requests each notify once while unfocused", async () => {
  notifyAttention(
    "approval-test",
    "Permission required",
    "Approve the command.",
  );
  notifyAttention(
    "approval-test",
    "Permission required",
    "Approve the command.",
  );
  notifyAttention("input-test", "Input needed", "Choose a project name.");
  await settle();
  expect(mocks.send).toHaveBeenCalledTimes(2);
  expect(mocks.send).toHaveBeenCalledWith({
    title: "Permission required",
    body: "Approve the command.",
  });
  expect(mocks.send).toHaveBeenCalledWith({
    title: "Input needed",
    body: "Choose a project name.",
  });
  expect(mocks.toast).not.toHaveBeenCalled();
});

test("request dialogs do not send a native notification while focused", async () => {
  mocks.focused.mockResolvedValue(true);
  notifyAttention(
    "focused-request-test",
    "Input needed",
    "Choose a project name.",
  );
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
});

test("returning to the app during a request permission check suppresses the notification", async () => {
  let finish!: (permission: boolean) => void;
  mocks.permission.mockReturnValue(
    new Promise<boolean>((resolve) => {
      finish = resolve;
    }),
  );
  notifyAttention(
    "focus-race-request-test",
    "Input needed",
    "Choose a project name.",
  );
  await settle();
  mocks.focused.mockResolvedValue(true);
  finish(true);
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
});

test("does not send if focus cannot be determined", async () => {
  mocks.focused.mockRejectedValue(new Error("window unavailable"));
  notify("Hello");
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
});

test("does not send when the app regains focus during the permission dialog", async () => {
  let finish!: (permission: string) => void;
  mocks.permission.mockResolvedValue(false);
  mocks.request.mockReturnValue(
    new Promise<string>((resolve) => {
      finish = resolve;
    }),
  );
  notify("Hello");
  await settle();
  expect(mocks.request).toHaveBeenCalledOnce();
  mocks.focused.mockResolvedValue(true);
  finish("granted");
  await settle();
  expect(mocks.send).not.toHaveBeenCalled();
});
