import assert from "node:assert/strict";
import test from "node:test";
import { createRoomProvider, attachHostAgent } from "./livekit-rooms.mjs";

const WALLET = "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC";
// endsAt is a wall-clock second, and a token is minted against the real clock.
const NOW = Math.floor(Date.now() / 1000);
const ENDS_AT = NOW + 3_600;

/** The two SDK clients, faked: they record calls and never touch a network. */
function fakeClients() {
  const created = [];
  const deleted = [];
  const participantRemovals = [];
  const dispatchCalls = [];
  const rooms = {
    async createRoom(options) {
      created.push(options);
      return { name: options.name };
    },
    async listRooms() {
      return created.map((room) => ({ ...room }));
    },
    async listParticipants() {
      return [{ identity: "agent-job-9" }, { identity: "guest-abcd" }];
    },
    async removeParticipant(roomName, identity) {
      participantRemovals.push([roomName, identity]);
    },
    async deleteRoom(roomName) {
      deleted.push(roomName);
    },
  };
  const dispatch = {
    async listDispatch() {
      // The unnamed one is the automatic agent, and it has a job running.
      return [{ id: "d1", state: { jobs: [{ id: "job-9" }] } }, { id: "d2", agentName: "someone-else" }];
    },
    async deleteDispatch(id) {
      dispatchCalls.push(["delete", id]);
    },
    async createDispatch(roomName, agentName) {
      dispatchCalls.push(["create", roomName, agentName]);
    },
  };
  return { rooms, dispatch, created, deleted, participantRemovals, dispatchCalls };
}

function provider(clients = fakeClients()) {
  return createRoomProvider({
    rooms: clients.rooms,
    dispatch: clients.dispatch,
    key: "api-key",
    secret: "api-secret",
    agentName: "archava-host",
  });
}

test("a room is labelled with its wallet, its end, and never as a preview", async () => {
  const clients = fakeClients();
  const p = provider(clients);
  const opened = await p.openRoom({ roomName: "archava-1", wallet: WALLET, endsAt: ENDS_AT });
  assert.equal(opened.roomName, "archava-1");

  assert.equal(clients.created.length, 1);
  const options = clients.created[0];
  assert.equal(options.name, "archava-1");
  assert.equal(options.maxParticipants, 3);
  const meta = JSON.parse(options.metadata);
  assert.deepEqual(meta, { product: "archava", wallet: WALLET, endsAt: ENDS_AT, preview: false });
});

test("opening a room attaches the named host agent and clears the automatic ones", async () => {
  const clients = fakeClients();
  await provider(clients).openRoom({ roomName: "archava-2", wallet: WALLET, endsAt: ENDS_AT });
  // The unnamed dispatch is the automatic one, so only it is deleted.
  assert.deepEqual(clients.dispatchCalls, [["delete", "d1"], ["create", "archava-2", "archava-host"]]);
  // The host agent is allowed to stay; the guest is never removed.
  assert.deepEqual(clients.participantRemovals, [["archava-2", "agent-job-9"]]);
});

test("a failed dispatch leaves no room behind", async () => {
  const clients = fakeClients();
  const failing = {
    ...clients.dispatch,
    async createDispatch() {
      throw new Error("no worker available");
    },
  };
  const p = createRoomProvider({
    rooms: clients.rooms,
    dispatch: failing,
    key: "api-key",
    secret: "api-secret",
    agentName: "archava-host",
  });
  await assert.rejects(() => p.openRoom({ roomName: "archava-3", wallet: WALLET, endsAt: ENDS_AT }), /no worker available/);
  assert.deepEqual(clients.deleted, ["archava-3"]);
});

test("deleting a room the server already dropped is not an error", async () => {
  const clients = fakeClients();
  clients.rooms = {
    ...clients.rooms,
    async deleteRoom() {
      const error = new Error("not_found");
      error.code = "not_found";
      throw error;
    },
  };
  await provider(clients).deleteRoom("archava-gone");
});

test("a token is minted for the room's wallet and expires with the room", async () => {
  const p = provider();
  const token = await p.issueToken({ roomName: "archava-4", wallet: WALLET, endsAt: ENDS_AT });
  assert.equal(typeof token, "string");
  const claims = JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString("utf8"));
  assert.equal(claims.iss, "api-key");
  assert.equal(claims.sub, WALLET);
  assert.equal(claims.video.room, "archava-4");
  assert.equal(claims.video.roomJoin, true);
  // A wallet token must announce itself as such, not as an anonymous guest.
  assert.equal(claims.video.roomCreate, undefined, "a customer webhook session joins, it does not create");
  // The token must not outlive the entitlement the room was opened with.
  assert.ok(claims.exp <= ENDS_AT, "token expiring at " + claims.exp);
  assert.ok(claims.exp > ENDS_AT - 300, "token should live as long as the room: " + claims.exp);
});

test("a room end that lies in the past cannot mint a token", async () => {
  const p = provider();
  await assert.rejects(() => p.issueToken({ roomName: "archava-5", wallet: WALLET, endsAt: NOW - 1 }), /ended room/);
  await assert.rejects(() => p.issueToken({ roomName: "archava-5", wallet: WALLET, endsAt: NOW }), /ended room/);
});

test("a room with less than 30 seconds left never receives a 30-second token", async () => {
  const p = provider();
  const endsAt = Math.floor(Date.now() / 1000) + 5;
  const token = await p.issueToken({ roomName: "archava-short", wallet: WALLET, endsAt });
  const claims = JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString("utf8"));
  assert.ok(claims.exp <= endsAt);
});

test("attachHostAgent refuses to guess which agent to dispatch", async () => {
  const clients = fakeClients();
  await assert.rejects(
    () => attachHostAgent({ roomName: "archava-6", rooms: clients.rooms, dispatch: clients.dispatch, agentName: "" }),
    /agent/,
  );
  assert.deepEqual(clients.dispatchCalls, []);
});
