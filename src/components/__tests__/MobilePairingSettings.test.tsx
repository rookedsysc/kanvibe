import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import MobilePairingSettings from "../MobilePairingSettings";

const mockStartMobilePairing = vi.fn();
const mockStopMobilePairing = vi.fn();
const mockListPairedMobileDevices = vi.fn();
const mockUnpairMobileDevice = vi.fn();

vi.mock("@/desktop/renderer/actions/mobileBridge", () => ({
  startMobilePairing: () => mockStartMobilePairing(),
  stopMobilePairing: () => mockStopMobilePairing(),
  listPairedMobileDevices: () => mockListPairedMobileDevices(),
  unpairMobileDevice: (deviceId: string) => mockUnpairMobileDevice(deviceId),
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
});

describe("MobilePairingSettings", () => {
  it("코드를 만들기 전에는 코드가 보이지 않는다", async () => {
    render(<MobilePairingSettings />);

    await waitFor(() => expect(mockListPairedMobileDevices).toHaveBeenCalled());
    expect(screen.queryByText("123456")).toBeNull();
  });

  it("버튼을 누르면 옮겨 적을 코드가 화면에 뜬다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "연결 코드 만들기" }));

    expect(await screen.findByText("123456")).toBeTruthy();
  });

  it("코드가 떠 있으면 지우는 버튼으로 바뀐다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "연결 코드 만들기" }));
    await screen.findByText("123456");

    expect(screen.queryByRole("button", { name: "연결 코드 만들기" })).toBeNull();
    expect(screen.getByRole("button", { name: "코드 지우기" })).toBeTruthy();
  });

  it("코드를 지우면 화면에서 사라진다", async () => {
    render(<MobilePairingSettings />);

    await userEvent.click(screen.getByRole("button", { name: "연결 코드 만들기" }));
    await screen.findByText("123456");
    await userEvent.click(screen.getByRole("button", { name: "코드 지우기" }));

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
    await userEvent.click(screen.getByRole("button", { name: "연결 끊기" }));

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
    await userEvent.click(screen.getByRole("button", { name: "연결 코드 만들기" }));
    await screen.findByText("123456");

    unmount();

    expect(mockStopMobilePairing).toHaveBeenCalled();
  });

  it("기기 목록을 못 읽어도 설정 화면은 그려진다", async () => {
    mockListPairedMobileDevices.mockRejectedValue(new Error("데스크탑 통로 없음"));

    render(<MobilePairingSettings />);

    expect(await screen.findByRole("button", { name: "연결 코드 만들기" })).toBeTruthy();
  });

  it("코드 발급이 실패하면 무엇이 안 됐는지 알려 준다", async () => {
    mockStartMobilePairing.mockRejectedValue(new Error("실패"));

    render(<MobilePairingSettings />);
    await userEvent.click(screen.getByRole("button", { name: "연결 코드 만들기" }));

    expect(await screen.findByText("연결 코드를 만들지 못했습니다.")).toBeTruthy();
  });
});
