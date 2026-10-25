import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import MobilePairingSettings from "../MobilePairingSettings";
import enMessages from "../../../messages/en.json";

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string, values?: Record<string, string>) => {
    const path = `${namespace}.${key}`.split(".");
    let node: unknown = enMessages;
    for (const segment of path) {
      node = (node as Record<string, unknown> | undefined)?.[segment];
    }

    return typeof node === "string"
      ? node.replace(/\{(\w+)\}/g, (_, name: string) => values?.[name] ?? `{${name}}`)
      : path.join(".");
  },
}));

const mockStartMobilePairing = vi.fn();
const mockStopMobilePairing = vi.fn();
const mockListPairedMobileDevices = vi.fn();
const mockUnpairMobileDevice = vi.fn();
const mockReadMobilePairingCode = vi.fn();

vi.mock("@/desktop/renderer/actions/mobileBridge", () => ({
  startMobilePairing: () => mockStartMobilePairing(),
  stopMobilePairing: () => mockStopMobilePairing(),
  listPairedMobileDevices: () => mockListPairedMobileDevices(),
  unpairMobileDevice: (deviceId: string) => mockUnpairMobileDevice(deviceId),
  readMobilePairingCode: () => mockReadMobilePairingCode(),
}));

const PAIRED_DEVICE = {
  deviceId: "d1",
  deviceName: "iPhone",
  pairedAt: "2026-09-08T00:00:00.000Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  mockStartMobilePairing.mockResolvedValue({ code: "123456", expiresAt: Date.now() + 300_000 });
  mockStopMobilePairing.mockResolvedValue(undefined);
  mockListPairedMobileDevices.mockResolvedValue([]);
  mockUnpairMobileDevice.mockResolvedValue(undefined);
  mockReadMobilePairingCode.mockResolvedValue("123456");
});

describe("MobilePairingSettings", () => {
  /**
   * 코드는 만료 말고도 사라진다. 실패가 상한에 닿으면 데스크탑이 코드를 버리는데,
   * 화면이 그것을 모르면 남은 시간이 흐르는 6자리가 그대로 남아 사용자가 원인을 찾지 못한다.
   */
  it("데스크탑이 버린 코드는 남은 시간이 있어도 화면에서 지운다", async () => {
    render(<MobilePairingSettings />);
    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));
    expect(await screen.findByText("123456")).toBeTruthy();

    mockReadMobilePairingCode.mockResolvedValue(null);

    await waitFor(() => expect(screen.queryByText("123456")).toBeNull(), { timeout: 3_000 });
  });

  it("코드를 만들기 전에는 코드가 보이지 않는다", async () => {
    render(<MobilePairingSettings />);

    await waitFor(() => expect(mockListPairedMobileDevices).toHaveBeenCalled());
    expect(screen.queryByText("123456")).toBeNull();
  });

  it("버튼을 누르면 옮겨 적을 코드가 화면에 뜬다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));

    expect(await screen.findByText("123456")).toBeTruthy();
  });

  it("코드가 떠 있으면 지우는 버튼으로 바뀐다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));
    await screen.findByText("123456");

    expect(screen.queryByRole("button", { name: "Create pairing code" })).toBeNull();
    expect(screen.getByRole("button", { name: "Clear code" })).toBeTruthy();
  });

  it("코드를 지우면 화면에서 사라진다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));
    await screen.findByText("123456");
    await userEvent.click(screen.getByRole("button", { name: "Clear code" }));

    await waitFor(() => expect(screen.queryByText("123456")).toBeNull());
    expect(mockStopMobilePairing).toHaveBeenCalled();
  });

  it("연결된 기기를 목록으로 보여 준다", async () => {
    mockListPairedMobileDevices.mockResolvedValue([PAIRED_DEVICE]);

    render(<MobilePairingSettings />);

    expect(await screen.findByText("iPhone")).toBeTruthy();
  });

  it("연결을 끊으면 그 기기 식별자로 요청하고 목록을 다시 읽는다", async () => {
    mockListPairedMobileDevices.mockResolvedValue([PAIRED_DEVICE]);

    render(<MobilePairingSettings />);
    await screen.findByText("iPhone");
    await userEvent.click(screen.getByRole("button", { name: "Disconnect" }));

    await waitFor(() => expect(mockUnpairMobileDevice).toHaveBeenCalledWith("d1"));
    expect(mockListPairedMobileDevices).toHaveBeenCalledTimes(2);
  });

  it("코드를 만든 적이 없으면 화면을 떠날 때 데스크탑을 부르지 않는다", async () => {
    const { unmount } = render(<MobilePairingSettings />);
    await waitFor(() => expect(mockListPairedMobileDevices).toHaveBeenCalled());

    unmount();

    expect(mockStopMobilePairing).not.toHaveBeenCalled();
  });

  it("코드를 띄워 둔 채 화면을 떠나면 그 코드를 거둔다", async () => {
    const { unmount } = render(<MobilePairingSettings />);
    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));
    await screen.findByText("123456");

    unmount();

    expect(mockStopMobilePairing).toHaveBeenCalled();
  });

  it("기기 목록을 못 읽어도 설정 화면은 그려진다", async () => {
    mockListPairedMobileDevices.mockRejectedValue(new Error("데스크탑 통로 없음"));

    render(<MobilePairingSettings />);

    expect(await screen.findByRole("button", { name: "Create pairing code" })).toBeTruthy();
  });

  it("코드 발급이 실패하면 무엇이 안 됐는지 알려 준다", async () => {
    mockStartMobilePairing.mockRejectedValue(new Error("실패"));

    render(<MobilePairingSettings />);
    await userEvent.click(screen.getByRole("button", { name: "Create pairing code" }));

    expect(await screen.findByText("Could not create a pairing code.")).toBeTruthy();
  });
});
