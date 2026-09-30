/**
 * Reads the single credential out of an Authorization header.
 *
 * Exactly one Bearer credential is accepted. Anything else — a second token, a
 * different scheme, or a token with whitespace inside it — returns null so the
 * caller fails closed rather than trying a broken one.
 */
export function bearerKey(authorizationHeader) {
  if (typeof authorizationHeader !== "string") return null;
  const match = /^\s*Bearer\s+(\S+)\s*$/i.exec(authorizationHeader);
  return match ? match[1] : null;
}
