/* Native browser media bindings. Business authority remains in the Rust host. */
(() => {
  'use strict';
  let current = null;
  let latestInput = "";
  const element = id => document.querySelector(`[data-testid="${id}"]`);
  const status = (text, state = 'idle') => {
    const node = element('voice-status');
    if (node) { node.textContent = text; node.dataset.state = state; }
  };
  const transcript = (role, text) => {
    const node = element('voice-transcript');
    if (!node || !text) return;
    node.textContent = `${node.textContent}\n${role}: ${text}`.slice(-12000);
  };
  const valid = active => current === active && !active.closed;
  const check = active => { if (!valid(active)) throw new Error('Session stopped.'); };
  const api = async (active, method, path, body) => {
    const response = await fetch(path, { method, credentials: 'same-origin',
      headers: { 'content-type': 'application/json', 'x-archava-csrf': active.csrf },
      body: body === undefined ? undefined : JSON.stringify(body) });
    const value = await response.json();
    if (!response.ok) throw new Error(value.message || value.error?.message || 'Voice is unavailable. Continue in chat.');
    return value;
  };
  async function cleanup(active) {
    active.closed = true;
    clearInterval(active.videoTimer);
    active.socket?.close();
    active.capture?.disconnect(); active.source?.disconnect();
    for (const source of active.playing || []) { try { source.stop(); } catch {} }
    for (const track of active.media?.getTracks() || []) track.stop();
    for (const track of active.destination?.stream.getTracks() || []) track.stop();
    const preview = element('voice-preview'); if (preview) preview.srcObject = null;
    try { await active.room?.disconnect(true); } catch {}
    try { await active.audio?.close(); } catch {}
    if (active.id) {
      try { await api(active, 'DELETE', `/presence/sessions/${active.id}`); } catch {}
    }
  }
  async function stop(message = 'Microphone and camera stopped. Continue in chat.') {
    const active = current; current = null;
    if (active) await cleanup(active);
    status(message);
  }
  function play(active, part) {
    const inline = part.inlineData;
    if (!inline?.mimeType?.startsWith('audio/pcm') || typeof inline.data !== 'string') return;
    if (inline.data.length > 1400000) throw new Error('Audio response exceeded its limit.');
    const bytes = Uint8Array.from(atob(inline.data), c => c.charCodeAt(0));
    if (bytes.length % 2) throw new Error('The audio response was malformed.');
    const view = new DataView(bytes.buffer);
    const rate = Number(/rate=(\d+)/.exec(inline.mimeType)?.[1] || 24000);
    if (![16000, 24000, 48000].includes(rate)) throw new Error('The audio sample rate is unsupported.');
    const buffer = active.audio.createBuffer(1, bytes.length / 2, rate);
    const samples = buffer.getChannelData(0);
    for (let i = 0; i < samples.length; i++) samples[i] = view.getInt16(i * 2, true) / 32768;
    const start = Math.max(active.audio.currentTime + .03, active.nextAudio || 0);
    if (start - active.audio.currentTime > 8) throw new Error('Audio playback is too far behind.');
    const source = active.audio.createBufferSource(); source.buffer = buffer;
    source.connect(active.audio.destination); source.connect(active.destination);
    active.playing.add(source); source.onended = () => active.playing.delete(source);
    source.start(start); active.nextAudio = start + buffer.duration;
    const node = element('voice-status'); if (node) node.dataset.receivedAudio = 'true';
  }
  async function start(tenant, csrf, camera) {
    if (current) await stop();
    latestInput = "";
    const active = { tenant, csrf, closed: false, playing: new Set() }; current = active;
    status('Connecting voice…', 'connecting');
    try {
      if (!window.LivekitClient || !navigator.mediaDevices?.getUserMedia) throw new Error('This browser cannot start realtime media. Continue in chat.');
      const session = await api(active, 'POST', '/presence/sessions', { tenant_id: tenant, mode: camera ? 'human' : 'voice' });
      active.id = session.session.session_id; check(active);
      if (session.media_transport !== 'gemini_live_pcm') throw new Error('A live media transport is not configured. Continue in chat.');
      active.room = new LivekitClient.Room({ adaptiveStream: true, dynacast: true });
      active.room.on(LivekitClient.RoomEvent.Disconnected, () => { if (valid(active)) void stop('The media room disconnected. Continue in chat.'); });
      await active.room.connect(session.livekit_url, session.token, { autoSubscribe: true }); check(active);
      const url = new URL(session.stream_path, location.origin); url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
      active.socket = new WebSocket(url);
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('Voice handshake timed out. Continue in chat.')), 12000);
        const finish = error => { clearTimeout(timer); error ? reject(error) : resolve(); };
        active.socket.onopen = () => { if (valid(active)) active.socket.send(JSON.stringify({ csrf })); else active.socket.close(); };
        active.socket.onerror = () => finish(new Error('The voice stream could not connect.'));
        active.socket.onclose = () => finish(new Error('The voice stream closed before connecting.'));
        active.socket.onmessage = event => { const message = JSON.parse(event.data); if (message.type === 'ready') finish(); else if (message.type === 'error') finish(new Error(message.message)); };
      }); check(active);
      active.audio = new AudioContext({ sampleRate: 16000 });
      await active.audio.resume(); check(active);
      active.destination = active.audio.createMediaStreamDestination();
      active.media = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }, video: camera ? { width: 640, height: 360, frameRate: 15 } : false });
      check(active);
      await active.room.localParticipant.publishTrack(active.media.getAudioTracks()[0], { name: 'operator-microphone', source: LivekitClient.Track.Source.Microphone }); check(active);
      await active.room.localParticipant.publishTrack(active.destination.stream.getAudioTracks()[0], { name: 'ava-voice', source: LivekitClient.Track.Source.Unknown }); check(active);
      await active.audio.audioWorklet.addModule('/v1/voice-worklet.js'); check(active);
      active.source = active.audio.createMediaStreamSource(active.media);
      active.capture = new AudioWorkletNode(active.audio, 'archava-pcm-capture');
      active.capture.port.onmessage = event => {
        if (!valid(active) || active.socket.readyState !== WebSocket.OPEN) return;
        if (active.socket.bufferedAmount > 131072) { void stop('The voice connection is too slow. Continue in chat.'); return; }
        active.socket.send(event.data);
      };
      active.source.connect(active.capture); active.capture.connect(active.audio.destination);
      active.socket.onmessage = event => {
        if (!valid(active)) return;
        try {
          const message = JSON.parse(event.data);
          if (message.type === 'closed' || message.type === 'error') { void stop(message.message); return; }
          const content = message.content; if (!content) return;
          if (content.interrupted) {
            for (const source of active.playing) { try { source.stop(); } catch {} }
            active.playing.clear(); active.nextAudio = 0;
          }
          if (content.inputTranscription?.text) { active.inputLine = ((active.inputLine || '') + ' ' + content.inputTranscription.text).trim().slice(-1000); latestInput = active.inputLine; }
          transcript('You', content.inputTranscription?.text);
          transcript('Ava', content.outputTranscription?.text);
          for (const part of content.modelTurn?.parts || []) { if (part.text) transcript('Ava', part.text); play(active, part); }
          if (content.turnComplete) active.inputLine = ''; 
        } catch { void stop('The voice response could not be played. Continue in chat.'); }
      };
      active.socket.onclose = () => { if (valid(active)) void stop('Voice disconnected. Continue in chat.'); };
      active.socket.onerror = () => { if (valid(active)) void stop('Voice disconnected. Continue in chat.'); };
      if (camera) {
        const track = active.media.getVideoTracks()[0];
        await active.room.localParticipant.publishTrack(track, { name: 'operator-camera', source: LivekitClient.Track.Source.Camera }); check(active);
        const preview = element('voice-preview'); preview.srcObject = active.media; await preview.play(); check(active);
        const canvas = document.createElement('canvas'); canvas.width = 640; canvas.height = 360;
        const context = canvas.getContext('2d');
        active.videoTimer = setInterval(() => {
          if (!valid(active) || active.socket.readyState !== WebSocket.OPEN || active.socket.bufferedAmount > 131072) return;
          context.drawImage(preview, 0, 0, 640, 360);
          const data = canvas.toDataURL('image/jpeg', .6).split(',')[1];
          if (data.length <= 262144) active.socket.send(JSON.stringify({ type: 'video', data }));
        }, 1000);
      }
      status(camera ? 'Voice and camera connected.' : 'Voice connected.', 'connected');
      const node = element('voice-status'); if (node) { node.dataset.livekit = active.room.state; node.dataset.session = active.id; }
    } catch (error) {
      if (current === active) { current = null; await cleanup(active); status(error.message || 'Voice is unavailable. Continue in chat.', 'unavailable'); }
      else await cleanup(active);
    }
  }
  window.ArchavaVoice = Object.freeze({ start, stop, getInput: () => current && valid(current) && current.csrf === localStorage.getItem('archava_csrf') ? latestInput : '' });
  window.addEventListener('pagehide', () => { void stop(); });
})();
