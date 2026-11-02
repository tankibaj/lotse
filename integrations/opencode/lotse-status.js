// Lotse status plugin for OpenCode.
//
// Reports OpenCode's session status to Lotse so the sidebar shows run / perm / fail / done for a pane.
// It never changes how OpenCode behaves, never reads or forwards conversation text, and never fails
// OpenCode: every error is swallowed. Outside a Lotse pane (no LOTSE_SESSION_ID) it does nothing.
//
// How it reports: for each interesting event it starts `lotse-hook opencode`, writes one JSON object
// to its stdin and moves on. `lotse-hook` forwards it to the `lotsed` Unix socket (token first).
// The payload contract (event names and fields) is documented in
// crates/lotse-detect/src/opencode.rs, which turns it into a status.
//
// Environment:
//   LOTSE_SESSION_ID  set by lotsed on every pane it spawns; without it the plugin is inert
//   LOTSE_HOOK_BIN    path to the lotse-hook binary (default: `lotse-hook` on PATH)
//
// Install (until `lotse-hook integration opencode install` exists): copy this file to
//   ${XDG_CONFIG_HOME:-~/.config}/opencode/plugins/lotse-status.js
// and remove it to uninstall. See README.md.
//
// API, from Orca's OpenCode status plugin (src/main/opencode/status-plugin-*-source.ts in
// /Users/naim/Repos/research/orca, read 2026-11-02): a plugin is `async (ctx) => hooks`, and the
// `event` hook receives `{ event }` with `event.type` and `event.properties`. Event names used:
// session.created / .status / .idle / .error / .deleted, permission.asked / .replied,
// question.asked / .replied / .rejected; `properties.sessionID`, `properties.info.{id,parentID,title}`,
// `properties.status.type` ("busy" | "retry" | "idle") and the request `id` are the fields Orca reads.
// UNVERIFIED against the OpenCode docs (https://opencode.ai/docs/plugins/; no web access when this was
// written): the fields of permission.asked that feed `tool` and `summary` (`permission`, `patterns`,
// and the older `type`, `pattern`, `title`), question.asked's `questions[0].question`, and the
// `session.error` error shape. All are optional here: a missing field only blanks the summary.
// OpenCode 1.x loads every function export; 2.x loads `default.server`. Both are exported, as Orca does.

import { spawn } from "node:child_process";

const MAX_TEXT = 160;
const KEEP_ALIVE_MS = 5 * 60 * 1000;

const clip = (value) => (typeof value === "string" ? value.slice(0, MAX_TEXT) : undefined);

const first = (value) => (Array.isArray(value) ? value.join(" ") : value);

// Events go out one at a time, in order: a `busy` must not overtake the `idle` before it. `lotse-hook`
// exits in milliseconds; a stuck one is killed after HOOK_TIMEOUT_MS, and a backlog beyond MAX_PENDING
// (Lotse is gone or wedged) drops new events instead of growing without bound.
const HOOK_TIMEOUT_MS = 3000;
const MAX_PENDING = 100;
let queue = Promise.resolve();
let pending = 0;

function deliver(payload) {
  return new Promise((resolve) => {
    try {
      const child = spawn(process.env.LOTSE_HOOK_BIN || "lotse-hook", ["opencode"], {
        stdio: ["pipe", "ignore", "ignore"],
        env: process.env,
      });
      const timer = setTimeout(() => {
        try {
          child.kill();
        } catch {}
        resolve();
      }, HOOK_TIMEOUT_MS);
      if (timer.unref) timer.unref();
      const done = () => {
        clearTimeout(timer);
        resolve();
      };
      child.on("error", done);
      child.on("close", done);
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(payload));
    } catch {
      // Lotse not installed or not running: never fail the agent.
      resolve();
    }
  });
}

function post(payload) {
  if (!process.env.LOTSE_SESSION_ID || pending >= MAX_PENDING) return;
  pending += 1;
  queue = queue
    .then(() => deliver(payload))
    .catch(() => {})
    .then(() => {
      pending -= 1;
    });
}

export const LotseStatusPlugin = async (_ctx) => {
  // sessionID -> parentID for child sessions (the task tool's sub-agents).
  const parents = new Map();
  // sessionID -> last status type sent, so repeated "busy" events are not forwarded.
  const lastStatus = new Map();

  // OpenCode sends no event while a long turn runs quietly, and Lotse treats a busy session that
  // stays silent for 30 minutes as lost. Re-announce busy sessions every few minutes (the adapter
  // ignores a repeat except for the clock). unref'd: it never keeps OpenCode alive.
  const keepAlive = setInterval(() => {
    for (const [id, type] of lastStatus) {
      if (type === "busy" || type === "retry") send("SessionStatus", id, { status: type });
    }
  }, KEEP_ALIVE_MS);
  if (keepAlive.unref) keepAlive.unref();

  const send = (name, sessionID, extra) => {
    if (typeof sessionID !== "string" || !sessionID) return;
    const parent = parents.get(sessionID);
    post({
      hook_event_name: name,
      session_id: sessionID,
      ...(parent ? { parent_id: parent } : {}),
      ...(extra || {}),
    });
  };

  const handle = (event) => {
    const props = event.properties || {};
    const sessionID = props.sessionID;
    switch (event.type) {
      case "session.created": {
        const info = props.info;
        if (!info || !info.id) return;
        if (info.parentID) parents.set(info.id, info.parentID);
        send("SessionCreated", info.id, { title: clip(info.title) });
        return;
      }
      case "session.deleted": {
        const id = props.info && props.info.id;
        send("SessionDeleted", id);
        parents.delete(id);
        lastStatus.delete(id);
        return;
      }
      case "session.status": {
        const type = props.status && props.status.type;
        if (type !== "busy" && type !== "retry" && type !== "idle") return;
        if (lastStatus.get(sessionID) === type) return;
        lastStatus.set(sessionID, type);
        send("SessionStatus", sessionID, { status: type });
        return;
      }
      case "session.idle":
        lastStatus.set(sessionID, "idle");
        send("SessionIdle", sessionID);
        return;
      case "session.error": {
        const error = props.error || {};
        // The user pressing Esc is a cancellation, not a failure (Orca treats it the same way).
        if (error.name === "MessageAbortedError") return;
        send("SessionError", sessionID, {
          message: clip((error.data && error.data.message) || error.name),
        });
        return;
      }
      case "permission.asked":
        send("PermissionAsked", sessionID, {
          request_id: props.id,
          tool: clip(props.permission || props.type),
          summary: clip(first(props.patterns || props.pattern) || props.title),
        });
        return;
      case "permission.replied":
        send("PermissionReplied", sessionID, {
          request_id: props.requestID || props.permissionID || props.id,
        });
        return;
      case "question.asked": {
        const question = Array.isArray(props.questions) ? props.questions[0] : undefined;
        send("QuestionAsked", sessionID, {
          request_id: props.id,
          summary: clip(question && question.question),
        });
        return;
      }
      case "question.replied":
      case "question.rejected":
        send(event.type === "question.replied" ? "QuestionReplied" : "QuestionRejected", sessionID, {
          request_id: props.requestID || props.id,
        });
        return;
      default:
    }
  };

  return {
    event: async ({ event }) => {
      try {
        if (event && event.type) handle(event);
      } catch {
        // never fail the agent
      }
    },
  };
};

export default {
  id: "lotse-status",
  server: LotseStatusPlugin,
};
