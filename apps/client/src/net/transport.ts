export type TransportState = "idle" | "connecting" | "ready" | "closed";
export type Channel = "reliable" | "unreliable";
export type SendResult = "queued" | "backpressured" | "closed";
type BytesHandler = (channel: Channel, bytes: Uint8Array) => void;

/** One connection attempt. Reconnect creates a fresh adapter and session. */
export class BrowserTransport {
  state: TransportState = "idle";
  private socket: WebSocket | undefined;
  private peer: RTCPeerConnection | undefined;
  private channel: RTCDataChannel | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private pressureTimer: ReturnType<typeof setInterval> | undefined;
  private pressureSince: number | undefined;
  private resolve: (() => void) | undefined;
  private reject: ((reason: Error) => void) | undefined;
  private readonly bytesHandlers = new Set<BytesHandler>();
  private readonly closeHandlers = new Set<(reason: string) => void>();

  constructor(private readonly timeoutMs = 15_000) {}
  onBytes(handler: BytesHandler): () => void {
    this.bytesHandlers.add(handler);
    return () => {
      this.bytesHandlers.delete(handler);
    };
  }
  onClose(handler: (reason: string) => void): () => void {
    this.closeHandlers.add(handler);
    return () => {
      this.closeHandlers.delete(handler);
    };
  }
  connect(url: string): Promise<void> {
    if (this.state !== "idle")
      return Promise.reject(new Error("Create a new transport to reconnect"));
    this.state = "connecting";
    return new Promise((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
      this.timer = setTimeout(
        () => this.close("WebRTC negotiation timed out"),
        this.timeoutMs,
      );
      try {
        this.peer = new RTCPeerConnection({ iceServers: [] });
        this.socket = new WebSocket(url);
        const socket = this.socket,
          peer = this.peer;
        socket.binaryType = "arraybuffer";
        let signalling = Promise.resolve();
        socket.onmessage = ({ data }: MessageEvent<unknown>) => {
          if (typeof data === "string") {
            signalling = signalling
              .then(async () => {
                if (this.isClosed()) return;
                if (data.length > 65_536)
                  throw new Error("Signalling too large");
                const message: unknown = JSON.parse(data);
                if (!message || typeof message !== "object")
                  throw new Error("Invalid signalling");
                if (
                  "sdp" in message &&
                  typeof message.sdp === "string" &&
                  message.sdp.length > 0
                ) {
                  await peer.setRemoteDescription({
                    type: "offer",
                    sdp: message.sdp,
                  });
                  if (this.isClosed()) return;
                  const answer = await peer.createAnswer();
                  if (this.isClosed()) return;
                  await peer.setLocalDescription(answer);
                  if (this.isClosed()) return;
                  this.signal({ type: "answer", sdp: answer.sdp });
                } else if (
                  "candidate" in message &&
                  message.candidate &&
                  typeof message.candidate === "object"
                ) {
                  await peer.addIceCandidate(
                    message.candidate as RTCIceCandidateInit,
                  );
                } else throw new Error("Unrecognized signalling");
              })
              .catch(() =>
                this.close("Invalid signalling or failed negotiation"),
              );
          } else this.receive("reliable", data);
        };
        socket.onclose = () => this.close("WebSocket closed");
        socket.onerror = () => this.close("WebSocket failed");
        peer.onicecandidate = ({ candidate }) => {
          if (candidate && this.state !== "closed")
            this.signal({ type: "ice", candidate: candidate.toJSON() });
        };
        peer.onconnectionstatechange = () => {
          if (
            peer.connectionState === "failed" ||
            peer.connectionState === "closed"
          )
            this.close(`WebRTC ${peer.connectionState}`);
        };
        peer.ondatachannel = ({ channel }) => {
          if (this.state === "closed") {
            channel.close();
            return;
          }
          if (
            this.channel ||
            channel.label !== "game" ||
            channel.ordered ||
            channel.maxRetransmits !== 0
          ) {
            channel.close();
            this.close("Unexpected RTC channel settings");
            return;
          }
          this.channel = channel;
          channel.binaryType = "arraybuffer";
          channel.onmessage = ({ data }: MessageEvent<unknown>) =>
            this.receive("unreliable", data);
          channel.onclose = () => this.close("RTC channel closed");
          channel.onerror = () => this.close("RTC channel failed");
          const opened = () => {
            if (this.state !== "connecting") return;
            this.state = "ready";
            clearTimeout(this.timer);
            this.pressureTimer = setInterval(() => this.checkPressure(), 100);
            this.resolve?.();
            this.resolve = undefined;
            this.reject = undefined;
          };
          channel.onopen = opened;
          if (channel.readyState === "open") opened();
        };
      } catch {
        this.close("Unable to create browser transport");
      }
    });
  }
  private isClosed(): boolean {
    return this.state === "closed";
  }
  private signal(message: object): void {
    if (this.socket?.readyState === WebSocket.OPEN && this.state !== "closed")
      this.socket.send(JSON.stringify(message));
  }
  private receive(channel: Channel, data: unknown): void {
    if (this.isClosed()) return;
    // A server hello may arrive just before the browser's RTC open callback.
    if (
      !(data instanceof ArrayBuffer) ||
      data.byteLength > (channel === "reliable" ? 16_384 : 1100)
    ) {
      this.close("Invalid binary application data");
      return;
    }
    try {
      for (const handler of this.bytesHandlers)
        handler(channel, new Uint8Array(data));
    } catch {
      this.close("Invalid server message");
    }
  }
  private checkPressure(): boolean {
    if ((this.socket?.bufferedAmount ?? 0) <= 256 * 1024) {
      this.pressureSince = undefined;
      return false;
    }
    this.pressureSince ??= performance.now();
    if (performance.now() - this.pressureSince >= 2000)
      this.close("Reliable connection stalled");
    return true;
  }
  send(channel: Channel, bytes: Uint8Array): SendResult {
    if (this.state !== "ready") return "closed";
    if (bytes.length > (channel === "reliable" ? 4096 : 1100))
      throw new Error("Client packet too large");
    if (
      channel === "reliable"
        ? this.checkPressure()
        : (this.channel?.bufferedAmount ?? Infinity) > 64 * 1024
    )
      return "backpressured";
    try {
      // Copy to an owned ArrayBuffer (also avoids SharedArrayBuffer browser overloads).
      const owned = new Uint8Array(bytes).buffer;
      if (channel === "reliable") this.socket?.send(owned);
      else this.channel?.send(owned);
      return "queued";
    } catch {
      this.close("Transport send failed");
      return "closed";
    }
  }
  close(reason = "Disconnected"): void {
    if (this.isClosed()) return;
    this.state = "closed";
    clearTimeout(this.timer);
    clearInterval(this.pressureTimer);
    if (this.channel) {
      this.channel.onopen =
        this.channel.onclose =
        this.channel.onerror =
        this.channel.onmessage =
          null;
      this.channel.close();
    }
    if (this.peer) {
      this.peer.onicecandidate =
        this.peer.ondatachannel =
        this.peer.onconnectionstatechange =
          null;
      this.peer.close();
    }
    if (this.socket) {
      this.socket.onopen =
        this.socket.onmessage =
        this.socket.onerror =
        this.socket.onclose =
          null;
      this.socket.close();
    }
    this.reject?.(new Error(reason));
    this.reject = undefined;
    this.resolve = undefined;
    for (const handler of this.closeHandlers) handler(reason);
    this.bytesHandlers.clear();
    this.closeHandlers.clear();
  }
}
