// Track preparation policy without fetching the model or contacting a provider.
export async function prepareSpatiusAvatar() {
  const response = await fetch("/__ui/avatar-preparation");
  if (!response.ok) throw new Error("Simulated avatar preparation failure");
  return {};
}
