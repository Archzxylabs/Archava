import { AvatarManager, AvatarSDK, DrivingServiceMode, type Avatar } from "@spatius/avatarkit";

let initializedAppId = "";
let initialization: Promise<void> | null = null;
const avatars = new Map<string, Promise<Avatar>>();

export async function prepareSpatiusAvatar(appId: string, avatarId: string): Promise<Avatar> {
  if (!("RTCRtpScriptTransform" in globalThis)) {
    throw new Error("This browser cannot display the Spatius avatar. Try a recent Chrome or Edge browser.");
  }
  if (!appId || !avatarId) throw new Error("Spatius avatar configuration is missing.");
  if (initializedAppId && initializedAppId !== appId) {
    throw new Error("The Spatius application changed. Reload this page to continue.");
  }

  if (!initialization) {
    initializedAppId = appId;
    initialization = AvatarSDK.initialize(appId, { drivingServiceMode: DrivingServiceMode.rtc })
      .catch((error: unknown) => {
        initialization = null;
        initializedAppId = "";
        throw error;
      });
  }
  await initialization;

  let loading = avatars.get(avatarId);
  if (!loading) {
    loading = AvatarManager.shared.load(avatarId).catch((error: unknown) => {
      avatars.delete(avatarId);
      throw error;
    });
    avatars.set(avatarId, loading);
  }
  return loading;
}
