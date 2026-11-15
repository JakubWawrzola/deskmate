// Shared by every property inspector of the plugin. The fields themselves are
// sdpi components and save their own values; this script adds what they do
// not have: the connection status, and the link between a dropdown filled by
// the plugin and the text field whose value is the setting.

(() => {
  // After the scope field changes, its value is saved 200 ms later; the
  // dropdown asks again once the plugin has the new value.
  const SCOPE_REFRESH_DELAY_MS = 500;

  const client = window.SDPIComponents?.streamDeckClient;
  if (!client) {
    // The components did not load, so there is nothing to talk through.
    return;
  }

  const statusText = document.getElementById("status-text");
  const statusHint = document.getElementById("status-hint");
  const directSettings = document.getElementById("direct-settings");
  const picker = document.getElementById("picker");
  const pickerItem = document.getElementById("picker-item");
  const pickerTarget = document.getElementById("picker-target");
  const pickerScope = document.getElementById("picker-scope");

  function showStatus(status) {
    statusText.textContent = status.text;
    statusHint.textContent = status.hint;
    statusHint.hidden = status.hint === "";
    if (directSettings) {
      // The address and token mean nothing while only the app is used.
      directSettings.hidden = status.mode === "deskmate";
    }
  }

  function showPicker(items) {
    // With nothing to pick from, the dropdown would only be in the way.
    pickerItem.hidden = !Array.isArray(items) || items.length === 0;
  }

  client.sendToPropertyInspector.subscribe((message) => {
    const payload = message.payload;
    if (payload?.event === "status") {
      showStatus(payload);
    } else if (pickerItem && payload?.event === picker?.getAttribute("datasource")) {
      showPicker(payload.items);
    }
  });

  if (picker && pickerTarget) {
    // The text field holds the setting. Picking an option types it in, and
    // the dropdown shows the option that matches what is typed.
    picker.addEventListener("valuechange", () => {
      if (picker.value) {
        pickerTarget.value = picker.value;
      }
    });
    pickerTarget.addEventListener("valuechange", () => {
      picker.value = pickerTarget.value;
    });
    picker.value = pickerTarget.value;
  }

  if (picker && pickerScope) {
    let refreshTimer;
    pickerScope.addEventListener("valuechange", () => {
      clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => picker.refresh(), SCOPE_REFRESH_DELAY_MS);
    });
  }

  client.send("sendToPlugin", { event: "getStatus" });
})();
