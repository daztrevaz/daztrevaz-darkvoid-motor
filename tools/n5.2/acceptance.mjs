// Real motor + external Qwen process. Test text and generated media stay outside Git.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";
import readline from "node:readline";
import { once } from "node:events";

const [requestPath, directory] = process.argv.slice(2);
if (!requestPath || !directory) throw new Error("Informe request JSON externo e pasta de saída no D:.");
const input = JSON.parse(await fs.readFile(requestPath, "utf8"));
const token = randomBytes(32).toString("hex");
const child = spawn(path.resolve("target/debug/darkvoid-motor.exe"), ["--token", token, "--data-dir", directory], { windowsHide: true, stdio: ["pipe", "pipe", "ignore"] });
let socket;
const final = new Map();
const progress = new Map();
const pending = new Map();
let seq = 0;
const deadline = Date.now() + 8 * 60 * 1000;
const request = (cmd, params = {}) => new Promise((resolve, reject) => {
  const id = ++seq;
  const timer = setTimeout(() => { pending.delete(id); reject(new Error(`Sem resposta: ${cmd}`)); }, 30000);
  pending.set(id, { resolve: (value) => { clearTimeout(timer); resolve(value); }, reject: (error) => { clearTimeout(timer); reject(error); } });
  socket.send(JSON.stringify({ id, cmd, params }));
});
const job = async (id) => {
  while (!final.has(id)) {
    if (Date.now() > deadline) throw new Error("Tempo de prova excedido.");
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return final.get(id);
};
const digest = async (file) => createHash("sha256").update(await fs.readFile(file)).digest("hex");
try {
  const lines = readline.createInterface({ input: child.stdout });
  const ready = await Promise.race([
    (async () => { for await (const line of lines) { try { const value = JSON.parse(line); if (value.event === "ready") return value; } catch {} } throw new Error("Motor encerrou antes de ficar pronto."); })(),
    new Promise((_, reject) => setTimeout(() => reject(new Error("Motor não iniciou.")), 30000).unref()),
  ]);
  socket = new WebSocket(`ws://127.0.0.1:${ready.port}/?token=${token}`);
  socket.addEventListener("message", ({ data }) => {
    if (typeof data !== "string") return; // JPEG preview frames are binary.
    const value = JSON.parse(data);
    if (value.id !== undefined) {
      const callback = pending.get(value.id);
      pending.delete(value.id);
      if (value.ok) callback?.resolve(value.result); else callback?.reject(new Error(value.error.message));
    } else if (value.event === "job.done" || value.event === "job.error") {
      final.set(value.data.jobId, value);
    } else if (value.event === "job.progress") {
      progress.set(value.data.jobId, (progress.get(value.data.jobId) ?? 0) + 1);
      if (value.data.stage) console.log(`Job ${value.data.jobId}: ${value.data.stage}`);
    }
  });
  await once(socket, "open");
  await request("hello");
  const voices = await request("speech.voices");
  assert(voices.voices.some((voice) => voice.id === input.profile && voice.available));
  const first = await request("speech.say", { text: input.text, profile: input.profile });
  const queued = await request("speech.say", { text: input.text, profile: input.profile });
  await request("job.cancel", { jobId: queued.jobId });
  assert.equal((await job(queued.jobId)).data.error.code, "cancelled");
  const done = await job(first.jobId);
  assert.equal(done.event, "job.done", JSON.stringify(done));
  assert((progress.get(first.jobId) ?? 0) >= 3);
  const original = done.data.result;
  assert.equal(await digest(original.path), original.sha256);
  const info = await request("media.probe", { path: original.path });
  assert.equal(info.kind, "audio");
  const clip = { id: "narration-acceptance", track: "A1", start: 0, in: 0, out: info.duration, media: original.path, volume: 1, fadeIn: 0, fadeOut: 0 };
  const timeline = await request("timeline.set", { format: "horizontal", clips: [clip] });
  assert(timeline.duration > 0);
  console.log("Geração real, progresso, cancelamento e inserção A1 aprovados.");
  const again = await request("speech.say", { text: input.text, profile: input.profile });
  const regenerated = await job(again.jobId);
  assert.equal(regenerated.event, "job.done", JSON.stringify(regenerated));
  const replacement = regenerated.data.result;
  assert.notEqual(original.path, replacement.path);
  const replacementInfo = await request("media.probe", { path: replacement.path });
  await request("timeline.set", { format: "horizontal", clips: [{ ...clip, media: replacement.path, out: replacementInfo.duration }] });
  assert.equal(await digest(original.path), original.sha256);
  const evidence = { provider: voices.provider, queueCancellation: true, progressEvents: progress.get(first.jobId), insertedAudio: info.kind, regenerationPreservedOriginal: true, original, replacement };
  await fs.writeFile(path.join(directory, "acceptance.json"), JSON.stringify(evidence, null, 2));
  console.log("Regeneração real, substituição e preservação do arquivo anterior aprovadas.");
} finally {
  socket?.close();
  child.stdin.end();
  await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 15000).unref())]);
  if (child.exitCode === null) child.kill();
}
