/** Owned LiveKit + wire-compatible Gemini fixture. No paid service is contacted. */
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { createHash } from 'node:crypto'
import { createSocket } from 'node:dgram'
import { createWriteStream, existsSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { spawn } from 'node:child_process'

function frame(text, opcode = 1) {
  const payload = Buffer.from(text)
  const size = payload.length < 126 ? 2 : payload.length <= 65535 ? 4 : 10
  const header = Buffer.alloc(size); header[0] = 128 | opcode
  if (size === 2) header[1] = payload.length
  else if (size === 4) { header[1] = 126; header.writeUInt16BE(payload.length, 2) }
  else { header[1] = 127; header.writeBigUInt64BE(BigInt(payload.length), 2) }
  return Buffer.concat([header, payload])
}
export async function startMediaFixture({ scratch, output, freePort, until, terminate }) {
  const binary = process.env.E2E_LIVEKIT_BIN && resolve(process.env.E2E_LIVEKIT_BIN)
  assert.ok(binary && existsSync(binary), 'ARCHAVA_E2E_MEDIA requires E2E_LIVEKIT_BIN pointing to an installed LiveKit server')
  const counts = { connections: 0, setups: 0, audioFrames: 0, audioBytes: 0, videoFrames: 0, videoBytes: 0, responses: 0, errors: [] }
  const sockets = new Set()
  const mock = createServer((_, response) => { response.writeHead(404); response.end() })
  mock.on('upgrade', (request, socket, head) => {
    if (new URL(request.url, 'http://127.0.0.1').searchParams.get('key') !== 'owned-gemini-key') { socket.destroy(); return }
    const accept = createHash('sha1').update(`${request.headers['sec-websocket-key']}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64')
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`)
    sockets.add(socket); counts.connections++; let buffer = Buffer.from(head), lastReply = 0, setup = false
    const receive = bytes => {
      buffer = Buffer.concat([buffer, bytes])
      try {
        while (buffer.length >= 2) {
          const opcode = buffer[0] & 15, masked = !!(buffer[1] & 128)
          let length = buffer[1] & 127, offset = 2
          if (length === 126) { if (buffer.length < 4) return; length = buffer.readUInt16BE(2); offset = 4 }
          else if (length === 127) { if (buffer.length < 10) return; const big = buffer.readBigUInt64BE(2); assert.ok(big <= 1048576n); length = Number(big); offset = 10 }
          assert.ok(masked && length <= 1048576)
          if (buffer.length < offset + 4 + length) return
          const mask = buffer.subarray(offset, offset + 4), payload = Buffer.from(buffer.subarray(offset + 4, offset + 4 + length))
          buffer = buffer.subarray(offset + 4 + length)
          for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4]
          if (opcode === 8) { socket.end(frame('', 8)); return }
          if (opcode === 9) { socket.write(frame(payload, 10)); continue }
          if (opcode === 10) continue
          assert.equal(opcode, 1)
          const message = JSON.parse(payload.toString())
          if (!setup) {
            assert.equal(message.setup.model, 'models/owned-live')
            assert.deepEqual(message.setup.generationConfig.responseModalities, ['AUDIO'])
            setup = true; counts.setups++; socket.write(frame(JSON.stringify({ setupComplete: {} }))); continue
          }
          const input = message.realtimeInput
          if (input?.audio) {
            assert.equal(input.audio.mimeType, 'audio/pcm;rate=16000')
            const pcm = Buffer.from(input.audio.data, 'base64'); assert.ok(pcm.length > 0 && pcm.length % 2 === 0)
            counts.audioFrames++; counts.audioBytes += pcm.length
            if (Date.now() - lastReply > 1000) {
              lastReply = Date.now(); const audio = Buffer.alloc(4800)
              for (let i = 0; i < audio.length / 2; i++) audio.writeInt16LE(Math.round(Math.sin(i / 24000 * 440 * 2 * Math.PI) * 2000), i * 2)
              socket.write(frame(JSON.stringify({ serverContent: { inputTranscription: { text: 'Review project milestones for my approval' }, outputTranscription: { text: 'Owned media fixture response' }, modelTurn: { parts: [{ inlineData: { mimeType: 'audio/pcm;rate=24000', data: audio.toString('base64') } }] }, turnComplete: true } })))
              counts.responses++
            }
          }
          if (input?.video) {
            assert.equal(input.video.mimeType, 'image/jpeg')
            const jpeg = Buffer.from(input.video.data, 'base64'); assert.ok(jpeg.length > 4 && jpeg[0] === 255 && jpeg[1] === 216)
            counts.videoFrames++; counts.videoBytes += jpeg.length
          }
        }
      } catch (error) { counts.errors.push(error.message); socket.destroy() }
    }
    socket.on('data', receive); socket.on('error', () => {}); socket.on('close', () => sockets.delete(socket))
    if (head.length) receive(Buffer.alloc(0))
  })
  await new Promise((resolve, reject) => { mock.once('error', reject); mock.listen(0, '127.0.0.1', resolve) })
  const signalPort = await freePort(), rtcPort = await freePort()
  const udp = createSocket('udp4'); await new Promise(r => udp.bind(0, '127.0.0.1', r)); const udpPort = udp.address().port; await new Promise(r => udp.close(r))
  const config = join(scratch, 'owned-livekit.yaml')
  writeFileSync(config, `port: ${signalPort}\nbind_addresses: [127.0.0.1]\nrtc:\n  node_ip: 127.0.0.1\n  use_external_ip: false\n  enable_loopback_candidate: true\n  udp_port: ${udpPort}\n  tcp_port: ${rtcPort}\n  stun_servers: []\nkeys:\n  owned-media-key: owned-media-secret-32-bytes-for-local-proof\nlogging:\n  level: info\nroom:\n  empty_timeout: 5\n  departure_timeout: 1\n`, { mode: 0o600 })
  const log = createWriteStream(join(output, 'owned-livekit.log'))
  const livekit = spawn(binary, ['--config', config], { stdio: ['ignore', 'pipe', 'pipe'] }); livekit.stdout.pipe(log); livekit.stderr.pipe(log)
  const close = async () => {
    for (const socket of sockets) socket.destroy()
    await new Promise(r => mock.close(r)); await terminate(livekit); await new Promise(r => log.end(r))
  }
  try {
    await until(async () => { if (livekit.exitCode !== null) throw Error(`Owned LiveKit exited ${livekit.exitCode}`); return (await fetch(`http://127.0.0.1:${signalPort}`)).ok }, 'owned LiveKit readiness')
  } catch (error) { await close(); throw error }
  return { counts, close, environment: { LIVEKIT_URL: `ws://127.0.0.1:${signalPort}`, LIVEKIT_API_KEY: 'owned-media-key', LIVEKIT_API_SECRET: 'owned-media-secret-32-bytes-for-local-proof', GEMINI_LIVE_API_KEY: 'owned-gemini-key', GEMINI_LIVE_MODEL: 'owned-live', GEMINI_LIVE_ENDPOINT: `ws://127.0.0.1:${mock.address().port}/live` } }
}
export async function verifyMedia({ page, origin, fixture, until, pass }) {
  const t = id => `[data-testid="${id}"]`
  await page.navigate(`${origin}/app/erp?module=voice`)
  await until(() => page.evaluate(`!!document.querySelector('${t('voice-start')}')`), 'native voice controls')
  await page.evaluate(`(()=>{window.__ownedMediaPCs=[];window.__ownedOriginalPC=window.RTCPeerConnection;window.RTCPeerConnection=class extends window.__ownedOriginalPC{constructor(...args){super(...args);window.__ownedMediaPCs.push(this);}};})()`)
  await page.click(t('voice-camera'))
  const box=await page.evaluate(`(()=>{const n=document.querySelector('${t('voice-start')}');n.scrollIntoView({block:'center'});const b=n.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`)
  await page.send('Input.dispatchMouseEvent',{type:'mousePressed',button:'left',clickCount:1,...box})
  await page.send('Input.dispatchMouseEvent',{type:'mouseReleased',button:'left',clickCount:1,...box})
  await until(() => page.evaluate(`document.querySelector('${t('voice-status')}')?.dataset.state==='connected'`), 'actual LiveKit room and Gemini media stream', 45000)
  await until(() => fixture.counts.audioFrames >= 2 && fixture.counts.videoFrames >= 1, 'microphone PCM and camera JPEG reach model fixture')
  await until(() => page.evaluate(`document.querySelector('${t('voice-status')}')?.dataset.receivedAudio==='true'`), 'model PCM is scheduled for browser playback')
  assert.ok((await page.text(t('voice-transcript'))).includes('Owned media fixture response'))
  const rtp=await until(()=>page.evaluate(`(async()=>{const reports=await Promise.all(window.__ownedMediaPCs.map(pc=>pc.getStats()));const outbound=reports.flatMap(report=>[...report.values()].filter(r=>r.type==='outbound-rtp').map(r=>({kind:r.kind,bytesSent:r.bytesSent||0})));return outbound.some(r=>r.kind==='audio'&&r.bytesSent>0)&&outbound.some(r=>r.kind==='video'&&r.bytesSent>0)?outbound:null;})()`),'actual audio/video RTP reaches the owned room')
  const connection = await page.evaluate(`({...document.querySelector('${t('voice-status')}').dataset})`)
  assert.equal(connection.livekit, 'connected')
  await page.screenshot('voice-and-camera-connected.png')
  let handoff=false
  if(process.env.ARCHAVA_E2E_AGENT_RUNTIME==='1'){
    await page.click(t('voice-plan-goal'))
    await until(()=>page.evaluate(`location.pathname==='/app/desk'&&document.querySelector('[data-testid=agent-conversation]')?.textContent.includes('Review project milestones for my approval')`),'spoken request reaches business planner')
    assert.ok((await page.text('[data-testid=agent-conversation]')).includes('Review project milestones for my approval'))
    assert.equal(await page.evaluate(`!!document.querySelector('[data-testid=agent-outcome]')`),false)
    handoff=true;pass('VOICE-agent-goal-handoff',{explicitUserTranscript:true,businessApprovalStillRequired:true})
  }else{
    await page.click(t('voice-stop'))
    await until(() => page.evaluate(`document.querySelector('${t('voice-status')}')?.dataset.state==='idle' && document.querySelector('${t('voice-preview')}')?.srcObject===null`), 'native stop releases media')
  }
  await until(()=>page.evaluate(`window.__ownedMediaPCs.every(pc=>pc.signalingState==='closed')`),'all owned WebRTC peer connections closed')
  await page.evaluate('window.RTCPeerConnection=window.__ownedOriginalPC')
  assert.equal(await page.evaluate('ArchavaVoice.getInput()'),'', 'Stopped media cannot hand off a prior session transcript')
  const sent = fixture.counts.audioFrames; await new Promise(r => setTimeout(r, 500)); assert.equal(fixture.counts.audioFrames, sent)
  assert.deepEqual(fixture.counts.errors, [])
  const result = { verified: true, handoff, connection, rtp, counts: { ...fixture.counts }, scope: 'Installed LiveKit server, actual Chromium WebRTC and PCM/JPEG over a wire-compatible owned Gemini fixture. Certificate-verified WSS is separately exercised by the Rust voice_media suite; no live provider acceptance.' }
  pass('VOICE-native-media', result)
  return result
}
