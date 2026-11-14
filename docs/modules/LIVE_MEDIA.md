# Conversational voice and camera

The native Voice screen at `/app/erp?module=voice` publishes a user's microphone
and optional camera to LiveKit and transports conversational audio/visual input
to Gemini Live. Model audio is played in the browser and published to the room;
input/output transcripts appear in the native screen. This interface is for
conversation and visual planning. Business changes continue through explicit
native review and approved missions.

## Configuration

Use [.env.example](../../.env.example) or
[the deployment template](../../deploy/archava.env.example). Configure a reachable
LiveKit URL and its server API key/secret, plus `GEMINI_LIVE_MODEL` and
`GEMINI_LIVE_API_KEY` (or the nonempty `GEMINI_API_KEY` fallback). The model is an
explicit operator selection rather than an assumed permanently available model.
Production uses TLS. Insecure transport is accepted only for owned loopback test
fixtures. The optional model WebSocket endpoint is server configuration; browser
clients cannot choose an upstream endpoint or receive the provider secret.

The server embeds the pinned LiveKit client 2.23.0, its unchanged upstream bundle,
license and provenance. No runtime CDN is required. CSP permits only the configured
validated LiveKit origin in addition to the application origin. Browser media
permissions and a user gesture are required; unavailable configuration refuses
rather than simulating a connection.

## Transport and lifecycle

An authenticated Presence session returns a LiveKit token and the same-origin
`/presence/sessions/{id}/stream` path. The WebSocket requires a CSRF first frame,
the exact participant/session, current membership and the selected mode before
connecting upstream. Membership/session checks repeat during the stream and
before provider output. One stream per session and 32 concurrent server streams
are allowed. Sessions are bounded to five minutes and 64 MiB, with audio/frame
size and rate limits. Invalid input or lost authority closes the stream.

The browser captures PCM at 16 kHz through an AudioWorklet, sends at most one JPEG
per second, plays 24 kHz model audio and handles interruptions. Stop or route
cleanup closes the model socket, LiveKit room and Presence session, stops all
camera/microphone tracks and closes the audio context. Chat remains available if
media fails.

## Evidence and external acceptance

[The Rust media test](../../crates/host/tests/voice_media.rs) uses an owned local
WSS server with an ephemeral CA, real certificate verification, PCM/JPEG input,
audio output and grant revocation. The Chromium media gate starts an owned
LiveKit server and Gemini wire fixture, uses synthetic camera/microphone devices,
and verifies transmitted RTP bytes, model audio playback, transcripts and closed
tracks/peer connections. This proves the implemented protocol and lifecycle.

It does not prove availability, quality, quota or billing of a real provider
account. Credentials, production TURN/network conditions and provider-live
acceptance remain environment-specific work. No provider request is charged by
these owned-fixture checks.

Protocol references: [Gemini Live API](https://aistudio.google.com/api-reference/live),
[WebSocket setup](https://aistudio.google.com/docs/live-api/get-started-websocket)
and [LiveKit connection documentation](https://docs.livekit.io/intro/basics/connect/).
