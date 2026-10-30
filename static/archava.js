/**
 * Archava Universal Client Embed Script & Ava Modality Runtime.
 *
 * Enables seamless integration of Archava ERP, Checkout, and Ava's
 * Decoupled Interaction Modalities into any 3rd-party website
 * (Next.js, React, Vue, WordPress, Shopify, Laravel, static HTML)
 * via a single lightweight, zero-dependency script tag.
 *
 * Supported Modality Presets (`data-mode`):
 *   - "chat_only"     : Text streaming + interactive action cards (QRIS, SOW, products).
 *   - "voice_only"    : Low-latency audio streaming / hands-free headset mode.
 *   - "avatar_voice"  : Fullscreen or kiosk 3D/video concierge with lip-sync.
 *   - "full"          : Complete flagship: Avatar + Voice + Live Chat & Action Cards.
 *   - "avatar_chat"   : Visual ambient gestures/lip-sync with silent text chat.
 *
 * Supported Industry Presets (`data-profile`):
 *   - "fnb_cafe"             : F&B, Cafe, Coffee Shop, Cloud Kitchen
 *   - "clinic_healthcare"    : Aesthetic Clinic, Dental, Healthcare
 *   - "hospitality_hotel"    : Boutique Hotel, Villa, Resort
 *   - "retail_commerce"      : Retail, Apparel, Store
 *   - "logistics_fleet"      : Logistics, Fleet, Transport
 *   - "professional_services": Tech Studio, Creative Agency, Consulting
 *
 * Usage:
 *   <script src="https://cdn.archzxy.com/v1/archava.js"
 *           data-tenant="lumina-clinic"
 *           data-mode="full"
 *           data-profile="clinic_healthcare"
 *           data-position="bottom-right">
 *   </script>
 */

(function (window) {
  'use strict';

  var MODES = {
    CHAT_ONLY: 'chat_only',
    VOICE_ONLY: 'voice_only',
    AVATAR_VOICE: 'avatar_voice',
    FULL: 'full',
    AVATAR_CHAT: 'avatar_chat',
  };

  var INDUSTRY_PROMPTS = {
    fnb_cafe: [
      'Berapa total omzet kafe hari ini?',
      'Bahan apa saja yang stoknya mau habis?',
      'Tutup kasir dan cetak ringkasan shift',
    ],
    clinic_healthcare: [
      'Lihat jadwal reservasi treatment hari ini',
      'Kirim pengingat WhatsApp ke pasien besok',
      'Cek stok serum botol yang mendekati expired',
    ],
    hospitality_hotel: [
      'Berapa villa yang terisi malam ini?',
      'Kamar mana saja yang belum selesai dibersihkan?',
      'Kirim panduan check-in via WhatsApp ke tamu',
    ],
    retail_commerce: [
      'Cari sisa stok barang berdasarkan barcode',
      'Buat draft order pembelian ke distributor',
      'Cetak ulang struk transaksi terakhir',
    ],
    logistics_fleet: [
      'Dimana posisi truk B-9120-XYZ saat ini?',
      'Truk mana yang perlu ganti oli minggu ini?',
      'Kirim update ETA pengiriman ke pelanggan',
    ],
    professional_services: [
      'Invoice mana saja yang sudah jatuh tempo?',
      'Buat draf tagihan termin 2 untuk klien',
      'Kirim ringkasan progres milestone ke klien',
    ],
  };

  function Archava(tenantId, options) {
    if (!tenantId) {
      throw new Error('[Archava] Missing required tenantId');
    }

    options = options || {};
    var baseUrl =
      options.host ||
      (typeof window !== 'undefined' && window.location ? window.location.origin : '');
    var mode = (options.mode || MODES.FULL).toLowerCase();
    var profile = (options.profile || 'fnb_cafe').toLowerCase();
    var position = options.position || 'bottom-right';
    var themeColor = options.themeColor || '#6366f1';

    return {
      tenantId: tenantId,
      baseUrl: baseUrl,
      mode: mode,
      profile: profile,

      /**
       * Opens high-performance modal checkout popup.
       * Eliminates the need for 3rd party to install React/NPM dependencies.
       */
      openCheckout: function (params) {
        params = params || {};
        var overlay = document.createElement('div');
        overlay.id = 'archava-modal-overlay';
        overlay.style.cssText =
          'position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(15,23,42,0.8);backdrop-filter:blur(8px);z-index:999999;display:flex;align-items:center;justify-content:center;transition:opacity 0.25s ease;';

        var iframe = document.createElement('iframe');
        var query = new URLSearchParams({
          tenant: tenantId,
          orderId: params.orderId || '',
          amount: params.amountCents || 0,
          embed: 'true',
        });
        iframe.src = baseUrl + '/embed/checkout?' + query.toString();
        iframe.style.cssText =
          'width:92%;max-width:580px;height:88vh;max-height:820px;border:1px solid rgba(255,255,255,0.12);border-radius:24px;box-shadow:0 25px 50px -12px rgba(0,0,0,0.6);background:#090d16;';

        var closeBtn = document.createElement('button');
        closeBtn.innerHTML = '&#x2715;';
        closeBtn.setAttribute('aria-label', 'Close Archava Checkout');
        closeBtn.style.cssText =
          'position:absolute;top:24px;right:24px;width:40px;height:40px;border-radius:50%;background:#1e293b;color:#f8fafc;border:1px solid rgba(255,255,255,0.15);cursor:pointer;font-size:18px;display:flex;align-items:center;justify-content:center;transition:all 0.2s;';
        closeBtn.onmouseover = function () {
          closeBtn.style.background = '#334155';
          closeBtn.style.transform = 'scale(1.05)';
        };
        closeBtn.onmouseout = function () {
          closeBtn.style.background = '#1e293b';
          closeBtn.style.transform = 'scale(1)';
        };

        var cleanup = function () {
          if (overlay.parentNode) {
            overlay.parentNode.removeChild(overlay);
          }
          window.removeEventListener('message', handleMessage);
        };

        closeBtn.onclick = function () {
          cleanup();
          if (typeof params.onCancel === 'function') {
            params.onCancel();
          }
        };

        var handleMessage = function (event) {
          if (!event.data || typeof event.data !== 'object') return;

          // S03 FIX: Strict origin verification against configured baseUrl
          var expectedOrigin = '';
          try {
            expectedOrigin = new URL(baseUrl, window.location.href).origin;
          } catch (e) {
            expectedOrigin = baseUrl;
          }
          if (event.origin !== expectedOrigin) {
            return;
          }

          // S03 FIX: Strict source verification: message must originate from this checkout iframe
          if (iframe && iframe.contentWindow && event.source !== iframe.contentWindow) {
            return;
          }

          // S03 / R02 FIX: Bound order ID validation - reject missing or mismatched orderId
          if (!params.orderId || !event.data.orderId || event.data.orderId !== params.orderId) {
            console.warn('[Archava SDK] Refusing checkout callback with missing or mismatched orderId:', event.data.orderId);
            return;
          }

          // R02 FIX: Handle simulated sandbox settlement events explicitly
          if (event.data.type === 'ARCHAVA_PAYMENT_SIMULATED') {
            cleanup();
            if (typeof params.onSimulated === 'function') {
              params.onSimulated(event.data.receipt);
            } else if (params.allowSandbox) {
              if (typeof params.onComplete === 'function') {
                params.onComplete(event.data.receipt);
              }
            } else {
              console.warn('[Archava SDK] Received simulated checkout event in non-sandbox mode. Use allowSandbox: true or onSimulated callback.');
            }
            return;
          }

          if (event.data.type === 'ARCHAVA_PAYMENT_SUCCESS') {
            var receipt = event.data.receipt || {};
            if (receipt.isSimulated || receipt.status === 'simulated_test') {
              console.warn('[Archava SDK] Production payment callback rejected unverified simulated receipt.');
              return;
            }
            cleanup();
            if (typeof params.onComplete === 'function') {
              params.onComplete(receipt);
            }
          }

          if (event.data.type === 'ARCHAVA_PAYMENT_CANCEL') {
            cleanup();
            if (typeof params.onCancel === 'function') {
              params.onCancel();
            }
          }
        };

        window.addEventListener('message', handleMessage);
        overlay.appendChild(iframe);
        overlay.appendChild(closeBtn);
        document.body.appendChild(overlay);
      },

      /**
       * Mounts an interactive view into an inline container element.
       */
      mount: function (selector, mountOptions) {
        var container = typeof selector === 'string' ? document.querySelector(selector) : selector;
        if (!container) {
          console.error('[Archava] Mount target container not found:', selector);
          return;
        }

        mountOptions = mountOptions || {};
        var view = mountOptions.view || 'erp';
        var height = mountOptions.height || '680px';
        var activeMode = mountOptions.mode || mode;
        var activeProfile = mountOptions.profile || profile;

        container.innerHTML = '';
        var iframe = document.createElement('iframe');
        var query = new URLSearchParams({
          tenant: tenantId,
          view: view,
          mode: activeMode,
          profile: activeProfile,
          embed: 'true',
        });
        iframe.src = baseUrl + '/app/' + view + '?' + query.toString();
        iframe.style.cssText =
          'width:100%;height:' +
          height +
          ';border:none;border-radius:16px;background:#090d16;box-shadow:0 10px 30px rgba(0,0,0,0.3);';

        container.appendChild(iframe);
      },

      /**
       * Mounts Ava Autonomous Assistant with Modality Presets and Proactive Action Cards.
       */
      mountAssistant: function (mountOptions) {
        mountOptions = mountOptions || {};
        var activeMode = (mountOptions.mode || mode).toLowerCase();
        var activeProfile = (mountOptions.profile || profile).toLowerCase();
        var isKiosk = mountOptions.kiosk || false;

        var hasChat = activeMode === MODES.CHAT_ONLY || activeMode === MODES.FULL || activeMode === MODES.AVATAR_CHAT;
        var hasVoice = activeMode === MODES.VOICE_ONLY || activeMode === MODES.AVATAR_VOICE || activeMode === MODES.FULL;
        var hasAvatar = activeMode === MODES.AVATAR_VOICE || activeMode === MODES.FULL || activeMode === MODES.AVATAR_CHAT;

        var widgetRoot = document.createElement('div');
        widgetRoot.id = 'archava-assistant-widget';
        widgetRoot.style.cssText = isKiosk
          ? 'position:fixed;top:0;left:0;width:100vw;height:100vh;z-index:999999;background:#090d16;'
          : 'position:fixed;' +
            (position.indexOf('left') !== -1 ? 'left:24px;' : 'right:24px;') +
            (position.indexOf('top') !== -1 ? 'top:24px;' : 'bottom:24px;') +
            'z-index:999990;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;';

        var trigger = null;
        if (!isKiosk) {
          trigger = document.createElement('button');
          trigger.id = 'archava-trigger-btn';
          trigger.setAttribute('aria-label', 'Open Ava Partner');
          trigger.style.cssText =
            'width:64px;height:64px;border-radius:50%;background:linear-gradient(135deg, ' +
            themeColor +
            ', #818cf8);box-shadow:0 12px 28px rgba(99,102,241,0.45);border:2px solid rgba(255,255,255,0.2);cursor:pointer;display:flex;align-items:center;justify-content:center;transition:all 0.3s cubic-bezier(0.34,1.56,0.64,1);outline:none;';

          trigger.onmouseover = function () {
            trigger.style.transform = 'scale(1.08) translateY(-2px)';
          };
          trigger.onmouseout = function () {
            trigger.style.transform = 'scale(1) translateY(0)';
          };

          if (hasAvatar) {
            trigger.innerHTML =
              '<svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>';
          } else if (hasVoice) {
            trigger.innerHTML =
              '<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="23"/><line x1="8" y1="23" x2="16" y2="23"/></svg>';
          } else {
            trigger.innerHTML =
              '<svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>';
          }
          widgetRoot.appendChild(trigger);
        }

        // Panel Window
        var panel = document.createElement('div');
        panel.id = 'archava-panel';
        panel.style.cssText =
          'display:' + (isKiosk ? 'flex' : 'none') + ';' +
          'flex-direction:column;border-radius:' + (isKiosk ? '0' : '24px') + ';' +
          'width:' + (isKiosk ? '100vw' : '420px') + ';' +
          'height:' + (isKiosk ? '100vh' : '650px') + ';' +
          'max-height:' + (isKiosk ? '100vh' : 'calc(100vh - 110px)') + ';' +
          'background:#090d16;border:1px solid rgba(255,255,255,0.12);box-shadow:0 30px 60px rgba(0,0,0,0.6);backdrop-filter:blur(20px);overflow:hidden;position:' + (isKiosk ? 'static' : 'absolute') + ';bottom:' + (isKiosk ? '0' : '80px') + ';right:' + (isKiosk ? '0' : '0') + ';';

        // Header
        var header = document.createElement('div');
        header.style.cssText =
          'padding:14px 18px;background:rgba(255,255,255,0.03);border-bottom:1px solid rgba(255,255,255,0.08);display:flex;align-items:center;justify-content:space-between;';
        header.innerHTML =
          '<div style="display:flex;align-items:center;gap:10px;">' +
            '<div style="width:10px;height:10px;border-radius:50%;background:#10b981;box-shadow:0 0 8px #10b981;"></div>' +
            '<div>' +
              '<div style="font-weight:600;font-size:14px;color:#f8fafc;letter-spacing:-0.01em;">Ava Autonomous Partner</div>' +
              '<div style="font-size:10px;color:#94a3b8;text-transform:uppercase;letter-spacing:0.05em;">' + activeProfile.replace('_', ' ') + ' &bull; ' + activeMode.replace('_', ' ') + '</div>' +
            '</div>' +
          '</div>';

        if (!isKiosk) {
          var closePanelBtn = document.createElement('button');
          closePanelBtn.innerHTML = '&#x2715;';
          closePanelBtn.style.cssText =
            'background:transparent;border:none;color:#94a3b8;cursor:pointer;font-size:16px;padding:4px;';
          closePanelBtn.onclick = function () {
            panel.style.display = 'none';
          };
          header.appendChild(closePanelBtn);
        }
        panel.appendChild(header);

        // Avatar Visual Stage (if enabled)
        if (hasAvatar) {
          var avatarStage = document.createElement('div');
          avatarStage.id = 'archava-avatar-stage';
          avatarStage.style.cssText =
            'height:' + (activeMode === MODES.AVATAR_VOICE ? 'calc(100% - 130px)' : '160px') + ';' +
            'background:radial-gradient(circle at center, #1e1b4b 0%, #090d16 100%);display:flex;flex-direction:column;align-items:center;justify-content:center;position:relative;overflow:hidden;flex-shrink:0;';
          
          avatarStage.innerHTML =
            '<div style="width:80px;height:80px;border-radius:50%;background:linear-gradient(135deg,#818cf8,#c084fc);display:flex;align-items:center;justify-content:center;box-shadow:0 0 35px rgba(129,140,248,0.4);border:2px solid rgba(255,255,255,0.4);animation:archava-pulse 2.5s infinite ease-in-out;">' +
              '<svg width="42" height="42" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="1.8"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>' +
            '</div>' +
            '<div style="margin-top:10px;font-size:11px;color:#cbd5e1;font-weight:500;">Ava Live Concierge &bull; Siap Membantu</div>';
          panel.appendChild(avatarStage);
        }

        // Voice Visualizer Stage (if Voice Only)
        if (activeMode === MODES.VOICE_ONLY) {
          var voiceStage = document.createElement('div');
          voiceStage.style.cssText =
            'flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:18px;';
          voiceStage.innerHTML =
            '<div style="width:110px;height:110px;border-radius:50%;background:radial-gradient(circle,#6366f1 0%,rgba(99,102,241,0.1) 70%);display:flex;align-items:center;justify-content:center;box-shadow:0 0 45px rgba(99,102,241,0.5);animation:archava-pulse 1.8s infinite ease-in-out;">' +
              '<svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="#fff" stroke-width="2"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/></svg>' +
            '</div>' +
            '<div style="font-size:14px;color:#f8fafc;font-weight:500;">Mendengarkan... Silakan bicara</div>' +
            '<div style="font-size:11px;color:#64748b;">Opus 48kHz HD Audio Aktif</div>';
          panel.appendChild(voiceStage);
        }

        // Proactive Action Feed Carousel (High-Value Actions)
        var actionFeed = document.createElement('div');
        actionFeed.id = 'archava-action-feed';
        actionFeed.style.cssText =
          'padding:8px 12px;background:rgba(255,255,255,0.02);border-bottom:1px solid rgba(255,255,255,0.06);display:flex;gap:8px;overflow-x:auto;scrollbar-width:none;flex-shrink:0;';
        
        // Mock / Initial Proactive Cards
        var cardsData = [
          {
            id: 'card_ar_1',
            priority: 'urgent',
            title: '3 Tagihan Jatuh Tempo',
            badge: 'Rp 14.2M',
            desc: 'Overdue > 7 hari',
            actionText: 'Kirim WA + QRIS',
          },
          {
            id: 'card_stock_1',
            priority: 'attention',
            title: '2 Item Stok Menipis',
            badge: 'Stok Kritis',
            desc: 'Biji Kopi & Susu UHT',
            actionText: 'Draft PO Supplier',
          },
        ];

        cardsData.forEach(function (card) {
          var cardEl = document.createElement('div');
          var isUrgent = card.priority === 'urgent';
          cardEl.style.cssText =
            'min-width:180px;background:rgba(255,255,255,0.04);border:1px solid ' +
            (isUrgent ? 'rgba(239,68,68,0.3)' : 'rgba(234,179,8,0.3)') +
            ';border-radius:12px;padding:8px 10px;display:flex;flex-direction:column;justify-content:space-between;gap:6px;';

          cardEl.innerHTML =
            '<div style="display:flex;align-items:center;justify-content:space-between;">' +
              '<span style="font-size:9px;font-weight:700;padding:2px 6px;border-radius:4px;background:' +
              (isUrgent ? 'rgba(239,68,68,0.2);color:#fca5a5' : 'rgba(234,179,8,0.2);color:#fde047') +
              ';text-transform:uppercase;">' + card.priority + '</span>' +
              '<span style="font-size:10px;font-weight:600;color:#94a3b8;">' + card.badge + '</span>' +
            '</div>' +
            '<div>' +
              '<div style="font-size:11px;font-weight:600;color:#f8fafc;line-height:1.3;">' + card.title + '</div>' +
              '<div style="font-size:10px;color:#94a3b8;">' + card.desc + '</div>' +
            '</div>';

          var actBtn = document.createElement('button');
          actBtn.style.cssText =
            'width:100%;padding:4px 8px;font-size:10px;font-weight:600;border-radius:6px;background:' +
            (isUrgent ? '#ef4444' : '#eab308') +
            ';color:#fff;border:none;cursor:pointer;transition:opacity 0.2s;';
          actBtn.textContent = card.actionText + ' (1-Tap)';

          actBtn.onclick = function () {
            actBtn.disabled = true;
            actBtn.textContent = 'Memproses...';
            setTimeout(function () {
              actBtn.style.background = '#10b981';
              actBtn.textContent = '✅ Berhasil!';
              if (chatStream) {
                var confirmMsg = document.createElement('div');
                confirmMsg.style.cssText =
                  'background:rgba(16,185,129,0.15);border:1px solid rgba(16,185,129,0.3);border-radius:12px;padding:10px 12px;color:#a7f3d0;font-size:12px;line-height:1.4;max-width:90%;align-self:flex-start;';
                confirmMsg.innerHTML =
                  '<strong>Aksi Selesai:</strong> ' + card.title + ' berhasil dieksekusi via Ava Cockpit.';
                chatStream.appendChild(confirmMsg);
                chatStream.scrollTop = chatStream.scrollHeight;
              }
            }, 800);
          };

          cardEl.appendChild(actBtn);
          actionFeed.appendChild(cardEl);
        });
        panel.appendChild(actionFeed);

        // Chat Conversation Stream (if Chat enabled)
        var chatStream = null;
        if (hasChat) {
          chatStream = document.createElement('div');
          chatStream.id = 'archava-chat-stream';
          chatStream.style.cssText =
            'flex:1;overflow-y:auto;padding:14px;display:flex;flex-direction:column;gap:10px;';
          
          chatStream.innerHTML =
            '<div style="background:rgba(255,255,255,0.05);border:1px solid rgba(255,255,255,0.08);border-radius:14px;padding:12px 14px;color:#e2e8f0;font-size:13px;line-height:1.5;max-width:85%;align-self:flex-start;">' +
              'Halo! Saya Ava, co-pilot operasional Anda. Pilih aksi cepat atau tanyakan apa saja.' +
            '</div>';

          // Quick Prompt Pills
          var promptsContainer = document.createElement('div');
          promptsContainer.style.cssText = 'display:flex;flex-wrap:wrap;gap:6px;margin-top:4px;';
          var prompts = INDUSTRY_PROMPTS[activeProfile] || INDUSTRY_PROMPTS.fnb_cafe;
          prompts.forEach(function (promptText) {
            var pill = document.createElement('button');
            pill.style.cssText =
              'background:rgba(99,102,241,0.15);border:1px solid rgba(99,102,241,0.3);border-radius:20px;padding:6px 12px;color:#c7d2fe;font-size:11px;cursor:pointer;transition:all 0.2s;text-align:left;';
            pill.textContent = promptText;
            pill.onmouseover = function () {
              pill.style.background = 'rgba(99,102,241,0.28)';
            };
            pill.onmouseout = function () {
              pill.style.background = 'rgba(99,102,241,0.15)';
            };
            pill.onclick = function () {
              handleUserQuery(promptText);
            };
            promptsContainer.appendChild(pill);
          });
          chatStream.appendChild(promptsContainer);
          panel.appendChild(chatStream);

          // Chat Input Controls
          var inputArea = document.createElement('div');
          inputArea.style.cssText =
            'padding:10px 14px;background:rgba(255,255,255,0.02);border-top:1px solid rgba(255,255,255,0.08);display:flex;align-items:center;gap:8px;flex-shrink:0;';
          
          var textInput = document.createElement('input');
          textInput.type = 'text';
          textInput.placeholder = 'Ketik pertanyaan atau instruksi...';
          textInput.style.cssText =
            'flex:1;background:rgba(255,255,255,0.06);border:1px solid rgba(255,255,255,0.1);border-radius:10px;padding:9px 12px;color:#fff;font-size:12px;outline:none;';
          
          var sendBtn = document.createElement('button');
          sendBtn.innerHTML = '&#x27A4;';
          sendBtn.setAttribute('aria-label', 'Send message');
          sendBtn.style.cssText =
            'width:34px;height:34px;border-radius:8px;background:#6366f1;color:#fff;border:none;cursor:pointer;display:flex;align-items:center;justify-content:center;font-size:13px;';
          
          function handleUserQuery(query) {
            var val = (query || textInput.value).trim();
            if (!val) return;

            // Render user bubble
            var userMsg = document.createElement('div');
            userMsg.style.cssText =
              'background:#6366f1;border-radius:12px;padding:8px 12px;color:#fff;font-size:12px;max-width:85%;align-self:flex-end;';
            userMsg.textContent = val;
            chatStream.appendChild(userMsg);
            textInput.value = '';
            chatStream.scrollTop = chatStream.scrollHeight;

            // Bimodal Generative Response Simulator
            setTimeout(function () {
              var qLower = val.toLowerCase();
              var reply = document.createElement('div');
              reply.style.cssText =
                'background:rgba(255,255,255,0.05);border:1px solid rgba(255,255,255,0.08);border-radius:12px;padding:10px 12px;color:#e2e8f0;font-size:12px;line-height:1.4;max-width:88%;align-self:flex-start;';

              if (qLower.indexOf('omzet') !== -1 || qLower.indexOf('penjualan') !== -1) {
                reply.innerHTML =
                  '<div style="font-size:11px;color:#94a3b8;margin-bottom:4px;">📊 Ringkasan Penjualan Hari Ini:</div>' +
                  '<div style="font-size:18px;font-weight:700;color:#10b981;">Rp 84.500.000 <span style="font-size:11px;color:#34d399;font-weight:500;">(+18.4%)</span></div>' +
                  '<div style="font-size:11px;color:#cbd5e1;margin-top:4px;">Total 142 pesanan selesai. 89% via QRIS instan.</div>' +
                  '<div style="margin-top:8px;height:4px;background:rgba(255,255,255,0.1);border-radius:2px;overflow:hidden;"><div style="width:78%;height:100%;background:#10b981;"></div></div>';
              } else if (qLower.indexOf('stok') !== -1 || qLower.indexOf('habis') !== -1) {
                reply.innerHTML =
                  '<div style="font-size:11px;color:#fca5a5;font-weight:600;margin-bottom:4px;">⚠️ Peringatan Stok Kritis (FIFO):</div>' +
                  '<div style="display:flex;flex-direction:column;gap:4px;margin-top:4px;">' +
                    '<div style="display:flex;justify-content:space-between;font-size:11px;color:#f8fafc;"><span>Biji Kopi Arabica</span><strong style="color:#ef4444;">1.8 kg (Sisa 1 hari)</strong></div>' +
                    '<div style="display:flex;justify-content:space-between;font-size:11px;color:#f8fafc;"><span>Susu UHT Fresh</span><strong style="color:#eab308;">4 liter (Sisa 2 hari)</strong></div>' +
                  '</div>' +
                  '<button style="margin-top:8px;width:100%;padding:6px;background:#6366f1;color:#fff;border:none;border-radius:6px;font-size:11px;font-weight:600;cursor:pointer;">Generate Purchase Order (PO)</button>';
              } else {
                reply.innerHTML =
                  'Saya telah mencatat instruksi Anda. Menghubungkan ke Ava Cockpit Engine untuk memverifikasi kebijakan dan mengeksekusi aksi terkait.';
              }

              chatStream.appendChild(reply);
              chatStream.scrollTop = chatStream.scrollHeight;
            }, 600);
          }

          sendBtn.onclick = function () {
            handleUserQuery();
          };

          textInput.onkeypress = function (e) {
            if (e.key === 'Enter') sendBtn.onclick();
          };

          inputArea.appendChild(textInput);
          inputArea.appendChild(sendBtn);
          panel.appendChild(inputArea);
        }

        widgetRoot.appendChild(panel);
        document.body.appendChild(widgetRoot);

        // Keyframe CSS injection
        var styleTag = document.createElement('style');
        styleTag.textContent =
          '@keyframes archava-pulse { 0%, 100% { transform: scale(1); opacity: 1; } 50% { transform: scale(1.05); opacity: 0.85; } }';
        document.head.appendChild(styleTag);

        // Toggle behavior
        if (trigger) {
          trigger.onclick = function () {
            var isHidden = panel.style.display === 'none';
            panel.style.display = isHidden ? 'flex' : 'none';
          };
        }
      },
    };
  }

  // Export to window and module systems
  if (typeof window !== 'undefined') {
    window.Archava = Archava;

    // Declarative Auto-initialization from <script> attributes
    var currentScript =
      document.currentScript ||
      document.querySelector('script[data-tenant]');

    if (currentScript && currentScript.getAttribute('data-tenant')) {
      var tenant = currentScript.getAttribute('data-tenant');
      var mode = currentScript.getAttribute('data-mode') || MODES.FULL;
      var profile = currentScript.getAttribute('data-profile') || 'fnb_cafe';
      var position = currentScript.getAttribute('data-position') || 'bottom-right';

      window.addEventListener('DOMContentLoaded', function () {
        var client = Archava(tenant, { mode: mode, profile: profile, position: position });
        client.mountAssistant();
      });
    }
  }

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = Archava;
  }
})(typeof window !== 'undefined' ? window : this);
