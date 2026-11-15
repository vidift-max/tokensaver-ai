import test from "node:test";
import assert from "node:assert/strict";
import { TeamRoomPhoneMissionAuthorizer } from "../scripts/phone-mission-authorizer.mjs";

const sender = "50000000-0000-4000-8000-000000000005";
const owner = "60000000-0000-4000-8000-000000000006";
const mission = "30000000-0000-4000-8000-000000000003";
const taskId = "40000000-0000-4000-8000-000000000004";
const identity = {
  principal_id: owner,
  participant_id: "70000000-0000-4000-8000-000000000007",
  session_id: "80000000-0000-4000-8000-000000000008",
  execution_id: "90000000-0000-4000-8000-000000000009",
  room_id: "a0000000-0000-4000-8000-00000000000a"
};
const task = () => ({
  task_id: taskId,
  status: "OWNED",
  owner_principal_id: identity.principal_id,
  owner_participant_id: identity.participant_id,
  owner_session_id: identity.session_id,
  owner_execution_id: identity.execution_id,
  fencing_token: 12,
  lease_expires_at: new Date(Date.now() + 60000).toISOString()
});
function setup({ lookup, authenticate, assertOwner } = {}) {
  const calls = [];
  const teamRoomClient = {
    authenticate: async () => { calls.push("authenticate"); return authenticate ? authenticate() : identity; },
    lookup: async id => { calls.push("lookup:" + id); return lookup ? lookup(id) : task(); },
    assertCurrentOwner: (t, i, fence) => {
      calls.push("assert-owner:" + fence);
      return assertOwner ? assertOwner(t, i, fence) : t;
    }
  };
  return {
    calls,
    authorizer: new TeamRoomPhoneMissionAuthorizer({
      teamRoomClient,
      authorizedSenderPrincipalId: sender
    })
  };
}
const message = () => ({ sender_principal_id: sender });
const envelope = () => ({ mission_id: mission, task_id: taskId });

test("requires a pinned coordinator principal UUID", () => {
  assert.throws(() => new TeamRoomPhoneMissionAuthorizer({
    teamRoomClient: {},
    authorizedSenderPrincipalId: "missing"
  }), e => e.code === "AUTHORIZER_CONFIG_REQUIRED");
});

test("authenticates executor and checks authoritative task ownership without acquiring", async () => {
  const { authorizer, calls } = setup();
  const result = await authorizer.authorize(message(), envelope());
  assert.equal(result.senderPrincipalId, sender);
  assert.equal(result.ownerPrincipalId, owner);
  assert.equal(result.fencingToken, 12);
  assert.deepEqual(calls, ["authenticate", "lookup:" + mission, "assert-owner:12"]);
});

test("rejects unpinned senders before contacting Team Room", async () => {
  const { authorizer, calls } = setup();
  await assert.rejects(
    () => authorizer.authorize({ sender_principal_id: owner }, envelope()),
    e => e.code === "AUTHORIZATION_LOST"
  );
  assert.deepEqual(calls, []);
});

test("rejects a task ID that does not match the authoritative mission", async () => {
  const { authorizer } = setup({ lookup: async () => ({ ...task(), task_id: "b0000000-0000-4000-8000-00000000000b" }) });
  await assert.rejects(() => authorizer.authorize(message(), envelope()), e => e.code === "AUTHORIZATION_LOST");
});

test("fails closed when Team Room reports lost ownership", async () => {
  const { authorizer } = setup({
    assertOwner: () => { const e = new Error("lease lost"); e.code = "LOSS_OF_OWNERSHIP"; throw e; }
  });
  await assert.rejects(() => authorizer.authorize(message(), envelope()), e => e.code === "AUTHORIZATION_LOST");
});
