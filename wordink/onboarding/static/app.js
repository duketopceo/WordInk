// WordInk Onboarding & Dashboard Frontend
document.addEventListener("DOMContentLoaded", () => {
    // State
    const state = {
        apiKey: "",
        selectedModel: "whisper-large-v3-turbo",
        selectedMicIndex: -1,
        hotkey: "alt+d",
        triggerMode: "toggle",
        isRecordingHotkey: false,
        meterInterval: null,
    };

    // DOM Elements - Navigation
    const navTabs = document.querySelectorAll(".nav-tab");
    const tabPanes = document.querySelectorAll(".tab-pane");

    // Wizard Steps
    const stepDots = document.querySelectorAll(".step-dot");
    const wizardSteps = document.querySelectorAll(".wizard-step");
    const btnNextList = document.querySelectorAll(".btn-next");
    const btnPrevList = document.querySelectorAll(".btn-prev");

    // Step 1: Key Validation
    const groqKeyInput = document.getElementById("groq-key");
    const btnValidateKey = document.getElementById("btn-validate-key");
    const keyStatus = document.getElementById("key-validation-status");
    const btnNext1 = document.getElementById("btn-next-1");

    // Step 2: Models
    const modelCards = document.querySelectorAll(".model-card");

    // Step 3: Mics & Meter
    const micSelect = document.getElementById("mic-select");
    const micWarning = document.getElementById("mic-warning");
    const meterBar = document.getElementById("meter-bar");
    const meterDb = document.getElementById("meter-db");
    const btnLiveTest = document.getElementById("btn-live-test");

    // Step 4: Key visualizer & Hotkey
    const liveKeyDisplay = document.getElementById("live-key-display");
    const liveKeyMeta = document.getElementById("live-key-meta");
    const activeHotkeyDisplay = document.getElementById("active-hotkey-display");
    const btnRecordHotkey = document.getElementById("btn-record-hotkey");
    const layoutTag = document.getElementById("keyboard-layout-tag");

    // Step 5: Save
    const btnSaveAll = document.getElementById("btn-save-all");
    const chkAutostart = document.getElementById("chk-autostart");

    // Tab 2: Stats
    const statWords = document.getElementById("stat-words");
    const statSessions = document.getElementById("stat-sessions");
    const statLatency = document.getElementById("stat-latency");
    const recentHistoryList = document.getElementById("recent-history-list");

    // Tab 3: Giant Visualizer
    const giantKeyText = document.getElementById("giant-key-text");
    const giantKeyDetail = document.getElementById("giant-key-detail");

    // --- TAB SWITCHING ---
    navTabs.forEach(tab => {
        tab.addEventListener("click", () => {
            const target = tab.dataset.tab;
            navTabs.forEach(t => t.classList.remove("active"));
            tabPanes.forEach(p => p.classList.remove("active"));
            tab.classList.add("active");
            const activePane = document.getElementById(`tab-${target}`);
            if (activePane) activePane.classList.add("active");

            if (target === "telemetry") {
                loadStats();
            }
        });
    });

    // --- WIZARD STEP NAVIGATION ---
    function goToStep(stepNum) {
        stepDots.forEach(dot => {
            const dotStep = parseInt(dot.dataset.step);
            dot.classList.toggle("active", dotStep === stepNum);
            dot.classList.toggle("completed", dotStep < stepNum);
        });
        wizardSteps.forEach(step => step.classList.remove("active"));
        const targetStep = document.getElementById(`step-${stepNum}`);
        if (targetStep) targetStep.classList.add("active");
    }

    btnNextList.forEach(btn => {
        btn.addEventListener("click", () => {
            const next = parseInt(btn.dataset.next);
            goToStep(next);
        });
    });

    btnPrevList.forEach(btn => {
        btn.addEventListener("click", () => {
            const prev = parseInt(btn.dataset.prev);
            goToStep(prev);
        });
    });

    // --- STEP 1: INITIAL STATUS & API KEY ---
    async function loadStatus() {
        try {
            const res = await fetch("/api/status");
            const data = await res.json();
            if (data.api_key_set) {
                keyStatus.className = "status-msg success";
                keyStatus.textContent = "✓ Groq API Key already configured.";
                btnNext1.disabled = false;
            }
            if (data.whisper_model) {
                state.selectedModel = data.whisper_model;
                updateSelectedModelUI();
            }
            if (data.hotkey) {
                state.hotkey = data.hotkey;
                activeHotkeyDisplay.textContent = formatHotkey(data.hotkey);
            }
        } catch (e) {
            console.warn("Could not load initial status", e);
        }
    }

    btnValidateKey.addEventListener("click", async () => {
        const key = groqKeyInput.value.trim();
        if (!key) {
            keyStatus.className = "status-msg error";
            keyStatus.textContent = "Please enter an API key.";
            return;
        }

        btnValidateKey.disabled = true;
        btnValidateKey.textContent = "Validating...";
        keyStatus.className = "status-msg info";
        keyStatus.textContent = "Connecting to Groq API...";

        try {
            const res = await fetch("/api/validate_key", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({ api_key: key })
            });
            const data = await res.json();
            if (res.ok && data.valid) {
                state.apiKey = key;
                keyStatus.className = "status-msg success";
                keyStatus.textContent = "✓ API Key valid! Whisper Turbo model verified.";
                btnNext1.disabled = false;
            } else {
                keyStatus.className = "status-msg error";
                keyStatus.textContent = `✗ Validation failed: ${data.error || "Invalid key"}`;
            }
        } catch (e) {
            keyStatus.className = "status-msg error";
            keyStatus.textContent = `✗ Network error: ${e.message}`;
        } finally {
            btnValidateKey.disabled = false;
            btnValidateKey.textContent = "Test & Validate";
        }
    });

    // --- STEP 2: MODEL SELECTION ---
    function updateSelectedModelUI() {
        modelCards.forEach(card => {
            card.classList.toggle("selected", card.dataset.model === state.selectedModel);
        });
    }

    modelCards.forEach(card => {
        card.addEventListener("click", () => {
            state.selectedModel = card.dataset.model;
            updateSelectedModelUI();
        });
    });

    // --- STEP 3: MICS & AUDIO METER ---
    async function loadMics() {
        try {
            const res = await fetch("/api/mics");
            const data = await res.json();
            micSelect.innerHTML = "";

            let hasFifineOrReal = false;
            (data.devices || []).forEach(d => {
                const opt = document.createElement("option");
                opt.value = d.index;
                opt.textContent = `${d.name} ${d.is_default ? "(Default)" : ""}`;
                if (d.name.toLowerCase().includes("fifine")) {
                    opt.selected = true;
                    hasFifineOrReal = true;
                } else if (d.is_default && !hasFifineOrReal) {
                    opt.selected = true;
                }
                micSelect.appendChild(opt);
            });

            if (micSelect.options.length === 0) {
                micWarning.style.display = "block";
                micWarning.textContent = "⚠️ No audio input devices detected.";
            } else {
                micWarning.style.display = "none";
            }
        } catch (e) {
            console.error("Error loading mics", e);
        }
    }

    let isTestingAudio = false;
    btnLiveTest.addEventListener("click", () => {
        isTestingAudio = !isTestingAudio;
        if (isTestingAudio) {
            btnLiveTest.textContent = "Stop Level Test";
            btnLiveTest.classList.add("btn-accent");
            state.meterInterval = setInterval(async () => {
                try {
                    const res = await fetch("/api/mic_test");
                    const data = await res.json();
                    const lvl = data.audio_level || 0;
                    meterBar.style.width = `${lvl}%`;
                    meterDb.textContent = lvl > 5 ? `Peak: ${lvl}% (Signal Active)` : `Listening... (${lvl}%)`;
                } catch (e) {
                    clearInterval(state.meterInterval);
                }
            }, 180);
        } else {
            btnLiveTest.textContent = "Start Level Test";
            btnLiveTest.classList.remove("btn-accent");
            clearInterval(state.meterInterval);
            meterBar.style.width = "0%";
            meterDb.textContent = "Speak into mic...";
        }
    });

    // --- STEP 4 & TAB 3: KEYBOARD DETECTION & HOTKEY BUILDER ---
    function formatKeyName(e) {
        // Human friendly key mappings
        const code = e.code;
        const key = e.key;

        if (code.startsWith("Key")) return code.replace("Key", "");
        if (code.startsWith("Digit")) return code.replace("Digit", "");
        if (code.startsWith("Numpad")) return `Numpad ${code.replace("Numpad", "")}`;
        if (code === "Space") return "Space";
        if (code === "Insert") return "Insert";
        if (code === "Delete") return "Delete";
        if (code === "Home") return "Home";
        if (code === "End") return "End";
        if (code === "PageUp") return "Page Up";
        if (code === "PageDown") return "Page Down";
        if (code.startsWith("F") && !isNaN(code.slice(1))) return code;
        return key.length === 1 ? key.toUpperCase() : key;
    }

    function formatHotkey(str) {
        return str.split("+").map(s => s.trim().charAt(0).toUpperCase() + s.trim().slice(1)).join(" + ");
    }

    // --- HOTKEY COLLISION SCANNER & RECOMMENDATIONS ---
    const btnRescanHotkeys = document.getElementById("btn-rescan-hotkeys");
    const hotkeyCandidatesList = document.getElementById("hotkey-candidates-list");

    async function loadHotkeyScan() {
        if (!hotkeyCandidatesList) return;
        hotkeyCandidatesList.innerHTML = `<div class="scanning-placeholder">Probing operating system hotkeys...</div>`;

        try {
            const res = await fetch("/api/scan_hotkeys");
            const data = await res.json();
            hotkeyCandidatesList.innerHTML = "";

            (data.hotkeys || []).forEach(item => {
                const chip = document.createElement("div");
                chip.className = `hotkey-chip ${item.available ? 'available' : 'conflict'} ${state.hotkey.toLowerCase() === item.hotkey.toLowerCase() ? 'selected' : ''}`;
                chip.innerHTML = `
                    <div class="chip-header">
                        <span class="chip-kbd">${item.label}</span>
                        <span class="chip-status-badge ${item.available ? 'available' : 'conflict'}">
                            ${item.available ? '✓ Available' : '✗ In Use'}
                        </span>
                    </div>
                    <div class="chip-desc">${item.desc} — ${item.reason}</div>
                `;
                if (item.available) {
                    chip.addEventListener("click", () => {
                        document.querySelectorAll(".hotkey-chip").forEach(c => c.classList.remove("selected"));
                        chip.classList.add("selected");
                        state.hotkey = item.hotkey;
                        activeHotkeyDisplay.textContent = formatHotkey(item.hotkey);
                    });
                }
                hotkeyCandidatesList.appendChild(chip);
            });
        } catch (e) {
            hotkeyCandidatesList.innerHTML = `<div class="error-msg">Could not probe hotkeys: ${e.message}</div>`;
        }
    }

    if (btnRescanHotkeys) {
        btnRescanHotkeys.addEventListener("click", loadHotkeyScan);
    }

    window.addEventListener("keydown", async (e) => {
        const prettyKey = formatKeyName(e);

        // Update Detected Key Visualizer
        const keyNameSpan = liveKeyDisplay.querySelector(".key-name");
        if (keyNameSpan) keyNameSpan.textContent = prettyKey;
        if (liveKeyMeta) liveKeyMeta.textContent = `Code: ${e.code} | Key: ${e.key} | Location: ${e.location}`;
        if (layoutTag) layoutTag.textContent = `Key: [ ${prettyKey} ] Detected`;

        // Update Giant Visualizer in Tab 3
        if (giantKeyText) giantKeyText.textContent = prettyKey;
        if (giantKeyDetail) giantKeyDetail.textContent = `Event Code: ${e.code} | Physical Scan: ${e.key} | Modifiers: ${[e.ctrlKey ? "Ctrl" : "", e.altKey ? "Alt" : "", e.shiftKey ? "Shift" : ""].filter(Boolean).join("+") || "None"}`;

        // If recording hotkey
        if (state.isRecordingHotkey) {
            e.preventDefault();
            const modifiers = [];
            if (e.ctrlKey) modifiers.push("ctrl");
            if (e.altKey) modifiers.push("alt");
            if (e.shiftKey) modifiers.push("shift");
            if (e.metaKey) modifiers.push("win");

            // Ignore bare modifier keydown until combination or trigger key pressed
            if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) {
                return;
            }

            const mainKey = prettyKey.toLowerCase();
            const fullCombo = [...modifiers, mainKey].join("+");

            // Probe server in real-time to check for conflicts
            try {
                const probeRes = await fetch("/api/probe_hotkey", {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ hotkey: fullCombo })
                });
                const probeData = await probeRes.json();
                if (!probeData.available) {
                    if (layoutTag) layoutTag.textContent = `⚠️ ${formatHotkey(fullCombo)} is ALREADY in use by another app!`;
                } else {
                    if (layoutTag) layoutTag.textContent = `✓ ${formatHotkey(fullCombo)} is available and set!`;
                }
            } catch (err) {}

            state.hotkey = fullCombo;
            activeHotkeyDisplay.textContent = formatHotkey(fullCombo);
            state.isRecordingHotkey = false;
            btnRecordHotkey.classList.remove("recording");
            btnRecordHotkey.innerHTML = `<span class="record-dot"></span> Click to Record New Hotkey`;
        }
    });

    btnRecordHotkey.addEventListener("click", () => {
        state.isRecordingHotkey = !state.isRecordingHotkey;
        if (state.isRecordingHotkey) {
            btnRecordHotkey.classList.add("recording");
            btnRecordHotkey.innerHTML = `<span class="record-dot pulse"></span> Press Desired Hotkey Now...`;
        } else {
            btnRecordHotkey.classList.remove("recording");
            btnRecordHotkey.innerHTML = `<span class="record-dot"></span> Click to Record New Hotkey`;
        }
    });

    // Mode selection
    const modeRadios = document.querySelectorAll('input[name="trigger_mode"]');
    modeRadios.forEach(radio => {
        radio.addEventListener("change", () => {
            state.triggerMode = radio.value;
            document.querySelectorAll(".toggle-option").forEach(opt => opt.classList.remove("selected"));
            radio.closest(".toggle-option").classList.add("selected");
        });
    });

    // --- STEP 5: SAVE & LAUNCH ---
    btnSaveAll.addEventListener("click", async () => {
        btnSaveAll.disabled = true;
        btnSaveAll.textContent = "Saving Configuration...";

        const payload = {
            api_key: state.apiKey || undefined,
            hotkey: state.hotkey,
            model: state.selectedModel,
            autostart: chkAutostart.checked
        };

        try {
            const res = await fetch("/api/save", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(payload)
            });
            const data = await res.json();
            if (res.ok && data.success) {
                btnSaveAll.textContent = "✓ Configured! Launching WordInk...";
                btnSaveAll.classList.add("btn-success");
                setTimeout(() => {
                    alert("🎉 WordInk is configured and ready!\n\nYour hotkey is: " + formatHotkey(state.hotkey) + "\nTap the hotkey in any application to dictate instantly!");
                }, 400);
            } else {
                alert(`Error saving configuration: ${data.error || "Unknown error"}`);
                btnSaveAll.disabled = false;
                btnSaveAll.textContent = "Save & Launch WordInk 🖋️";
            }
        } catch (e) {
            alert(`Network error saving configuration: ${e.message}`);
            btnSaveAll.disabled = false;
            btnSaveAll.textContent = "Save & Launch WordInk 🖋️";
        }
    });

    // --- TAB 2: TELEMETRY LOADER ---
    async function loadStats() {
        try {
            const res = await fetch("/api/stats");
            const data = await res.json();
            const s = data.stats || {};
            statWords.textContent = (s.total_words || 0).toLocaleString();
            statSessions.textContent = (s.total_sessions || 0).toLocaleString();
            statLatency.textContent = s.avg_latency ? `${s.avg_latency.toFixed(2)}s` : "~300ms";

            recentHistoryList.innerHTML = "";
            const recent = data.recent || [];
            if (recent.length === 0) {
                recentHistoryList.innerHTML = `<div class="empty-state">No transcriptions recorded yet. Press ${formatHotkey(state.hotkey)} to start!</div>`;
            } else {
                recent.forEach(item => {
                    const row = document.createElement("div");
                    row.className = "history-item";
                    row.innerHTML = `
                        <div class="history-meta">
                            <span class="history-time">${new Date(item.timestamp).toLocaleTimeString()}</span>
                            <span class="history-badge">${item.word_count || 0} words</span>
                            <span class="history-latency">${item.latency ? item.latency.toFixed(2) + 's' : ''}</span>
                        </div>
                        <div class="history-text">"${item.text}"</div>
                    `;
                    recentHistoryList.appendChild(row);
                });
            }
        } catch (e) {
            console.warn("Could not load stats", e);
        }
    }

    // Init
    loadStatus();
    loadMics();
    loadHotkeyScan();
});

