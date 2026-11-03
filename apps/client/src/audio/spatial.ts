/** Firefox exposes legacy listener methods where AudioParam properties are absent. */
type PositionNode = {
  positionX?: { value: number };
  positionY?: { value: number };
  positionZ?: { value: number };
  setPosition?: (x: number, y: number, z: number) => void;
};
export function setAudioPosition(
  node: PositionNode,
  position: readonly number[],
): void {
  const [x = 0, y = 0, z = 0] = position;
  if (node.positionX && node.positionY && node.positionZ) {
    node.positionX.value = x;
    node.positionY.value = y;
    node.positionZ.value = z;
  } else node.setPosition?.(x, y, z);
}
export function setAudioOrientation(
  listener: Partial<
    Pick<
      AudioListener,
      | "forwardX"
      | "forwardY"
      | "forwardZ"
      | "upX"
      | "upY"
      | "upZ"
      | "setOrientation"
    >
  >,
  yaw: number,
): void {
  const x = -Math.sin(yaw),
    z = -Math.cos(yaw);
  if (
    listener.forwardX &&
    listener.forwardY &&
    listener.forwardZ &&
    listener.upX &&
    listener.upY &&
    listener.upZ
  ) {
    listener.forwardX.value = x;
    listener.forwardY.value = 0;
    listener.forwardZ.value = z;
    listener.upX.value = 0;
    listener.upY.value = 1;
    listener.upZ.value = 0;
  } else listener.setOrientation?.(x, 0, z, 0, 1, 0);
}
