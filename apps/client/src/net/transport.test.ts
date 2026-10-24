import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserTransport } from "./transport";

class FakeSocket {
  static OPEN = 1;
  static latest: FakeSocket;
  readyState = 1;
  bufferedAmount = 0;
  binaryType = "";
  sent: unknown[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor() {
    FakeSocket.latest = this;
  }
  send(data: unknown) {
    this.sent.push(data);
  }
  close = vi.fn(() => {
    this.readyState = 3;
    this.onclose?.();
  });
}
class FakeChannel {
  label = "game";
  ordered = false;
  maxRetransmits = 0;
  readyState = "open";
  binaryType = "";
  bufferedAmount = 0;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  send = vi.fn();
  close = vi.fn(() => this.onclose?.());
}
class FakePeer {
  static latest: FakePeer;
  connectionState = "new";
  onicecandidate: unknown = null;
  onconnectionstatechange: (() => void) | null = null;
  ondatachannel: ((event: { channel: FakeChannel }) => void) | null = null;
  setRemoteDescription = vi.fn(async () => {});
  createAnswer = vi.fn(async () => ({ type: "answer", sdp: "answer" }));
  setLocalDescription = vi.fn(async () => {});
  addIceCandidate = vi.fn(async () => {});
  constructor() {
    FakePeer.latest = this;
  }
  close = vi.fn(() => {
    this.connectionState = "closed";
    this.onconnectionstatechange?.();
  });
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
  vi.stubGlobal("RTCPeerConnection", FakePeer);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
async function connected() {
  const transport = new BrowserTransport();
  const ready = transport.connect("ws://127.0.0.1:3000");
  const channel = new FakeChannel();
  FakePeer.latest.ondatachannel?.({ channel });
  await ready;
  return {
    transport,
    channel,
    socket: FakeSocket.latest,
    peer: FakePeer.latest,
  };
}
describe("BrowserTransport failure boundaries", () => {
  it("does not become ready from WS alone and times out at 15 seconds", async () => {
    const transport = new BrowserTransport();
    const closed = vi.fn();
    transport.onClose(closed);
    const result = transport.connect("ws://localhost").catch((e: unknown) => e);
    expect(transport.send("reliable", new Uint8Array([1]))).toBe("closed");
    await vi.advanceTimersByTimeAsync(14_999);
    expect(transport.state).toBe("connecting");
    await vi.advanceTimersByTimeAsync(1);
    expect(await result).toBeInstanceOf(Error);
    expect(closed).toHaveBeenCalledExactlyOnceWith(
      "WebRTC negotiation timed out",
    );
    expect(vi.getTimerCount()).toBe(0);
  });
  it("closes both transports exactly once and removes handlers on duplicate close", async () => {
    const { transport, socket, peer, channel } = await connected();
    const closed = vi.fn();
    transport.onClose(closed);
    transport.close("left");
    transport.close("again");
    expect(closed).toHaveBeenCalledExactlyOnceWith("left");
    expect(socket.close).toHaveBeenCalledOnce();
    expect(peer.close).toHaveBeenCalledOnce();
    expect(channel.close).toHaveBeenCalledOnce();
    expect(socket.onmessage).toBeNull();
    expect(channel.onmessage).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("backpressures RTC immediately and closes a stalled WS after two seconds", async () => {
    const { transport, socket, channel } = await connected();
    channel.bufferedAmount = 65_537;
    expect(transport.send("unreliable", new Uint8Array([1]))).toBe(
      "backpressured",
    );
    expect(channel.send).not.toHaveBeenCalled();
    socket.bufferedAmount = 262_145;
    expect(transport.send("reliable", new Uint8Array([1]))).toBe(
      "backpressured",
    );
    await vi.advanceTimersByTimeAsync(2000);
    expect(transport.state).toBe("closed");
  });
  it("rejects extra channels and nonbinary RTC packets", async () => {
    const { transport, channel } = await connected();
    channel.onmessage?.({ data: "JSON is not game data" });
    expect(transport.state).toBe("closed");
    const second = await connected();
    FakePeer.latest.ondatachannel?.({ channel: new FakeChannel() });
    expect(second.transport.state).toBe("closed");
  });
  it("keeps binary data separate from signalling and unsubscribes", async () => {
    const { transport, socket, channel } = await connected();
    const bytes = vi.fn();
    const off = transport.onBytes(bytes);
    socket.onmessage?.({ data: new Uint8Array([0x81, 0xff]).buffer });
    channel.onmessage?.({ data: new Uint8Array([0x82, 0xfe]).buffer });
    expect(bytes.mock.calls.map((call) => call[0])).toEqual([
      "reliable",
      "unreliable",
    ]);
    off();
    channel.onmessage?.({ data: new ArrayBuffer(1) });
    expect(bytes).toHaveBeenCalledTimes(2);
    transport.close();
  });
  it("serializes candidate processing behind the remote offer", async () => {
    const { transport, socket, peer } = await connected();
    const order: string[] = [];
    let release: (() => void) | undefined;
    peer.setRemoteDescription.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = () => {
            order.push("offer");
            resolve();
          };
        }),
    );
    peer.addIceCandidate.mockImplementation(async () => {
      order.push("candidate");
    });
    socket.onmessage?.({
      data: JSON.stringify({ sdp: "offer", candidate: null }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        sdp: null,
        candidate: { candidate: "candidate" },
      }),
    });
    await Promise.resolve();
    expect(order).toEqual([]);
    release?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(order).toEqual(["offer", "candidate"]);
    transport.close();
  });
});

it("bounds signalling retained behind a stalled offer and ignores its completion after close", async () => {
  const { transport, socket, peer } = await connected();
  let release: (() => void) | undefined;
  peer.setRemoteDescription.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  socket.onmessage?.({ data: JSON.stringify({ sdp: "stalled" }) });
  await Promise.resolve();
  for (let i = 0; i < 32; i++)
    socket.onmessage?.({
      data: JSON.stringify({ candidate: { candidate: `ice-${i}` } }),
    });
  expect(transport.state).toBe("closed");
  release?.();
  await vi.advanceTimersByTimeAsync(0);
  expect(peer.createAnswer).not.toHaveBeenCalled();
  expect(peer.addIceCandidate).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});

it("rejects oversized signalling before retaining it and permits pressure recovery", async () => {
  const first = await connected();
  first.channel.bufferedAmount = 65_537;
  for (let i = 0; i < 1000; i++)
    expect(first.transport.send("unreliable", new Uint8Array([1]))).toBe(
      "backpressured",
    );
  expect(first.channel.send).not.toHaveBeenCalled();
  first.channel.bufferedAmount = 0;
  expect(first.transport.send("unreliable", new Uint8Array([2]))).toBe(
    "queued",
  );
  first.socket.bufferedAmount = 262_145;
  await vi.advanceTimersByTimeAsync(1000);
  first.socket.bufferedAmount = 0;
  await vi.advanceTimersByTimeAsync(1500);
  expect(first.transport.state).toBe("ready");
  first.socket.onmessage?.({ data: "x".repeat(65_537) });
  expect(first.transport.state).toBe("closed");
});
