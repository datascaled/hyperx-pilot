<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import H1 from "@/components/H1.vue";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Locale, messages } from "@/i18n";

const LEGACY_LOCALE_STORAGE_KEY = "hyperx:locale";
const APP_SETTINGS_STORAGE_KEY = "hyperx:settings";
const APP_SETTINGS_VERSION = 1;
const DEVICE_SCAN_INTERVAL_MS = 3000;

interface DeviceOption {
  id: string;
  label: string;
}

interface DeviceSettings {
  sidetuneEnabled?: boolean;
  virtualSurroundEnabled?: boolean;
}

interface AppSettings {
  version: number;
  locale: Locale;
  selectedDeviceId: string | null;
  devices: Record<string, DeviceSettings>;
}

const { t, locale } = useI18n();

const sidetuneEnabled = ref(false);
const virtualSurroundEnabled = ref(false);
const devices = ref<DeviceOption[]>([]);
const devicesLoading = ref(true);
const deviceError = ref<string | null>(null);
const selectedDeviceId = ref<string | null>(null);
const sidetoneBusy = ref(false);
const sidetoneError = ref<string | null>(null);
const virtualSurroundBusy = ref(false);
const virtualSurroundError = ref<string | null>(null);

const deviceSelection = computed<string>({
  get: () => selectedDeviceId.value ?? "",
  set: (value) => {
    selectedDeviceId.value = value || null;
  },
});

const canToggleSidetone = computed(
  () => !!selectedDeviceId.value && !devicesLoading.value && !sidetoneBusy.value
);

const canToggleVirtualSurround = computed(
  () =>
    !!selectedDeviceId.value &&
    !devicesLoading.value &&
    !virtualSurroundBusy.value
);

const selectedLocale = computed<Locale>({
  get: () => locale.value as Locale,
  set: (value) => {
    locale.value = value;
    persistLocale(value);
  },
});

onMounted(() => {
  loadPersistedSettings();
  void loadDevices();
  if (typeof window === "undefined") return;
  deviceScanTimer = window.setInterval(() => {
    void loadDevices({ silent: true });
  }, DEVICE_SCAN_INTERVAL_MS);
});

onUnmounted(() => {
  if (typeof window === "undefined") return;
  if (deviceScanTimer === null) return;
  window.clearInterval(deviceScanTimer);
  deviceScanTimer = null;
});

function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return "Unknown error";
  }
}

function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && value in messages;
}

function defaultSettings(): AppSettings {
  return {
    version: APP_SETTINGS_VERSION,
    locale: "en",
    selectedDeviceId: null,
    devices: {},
  };
}

function parseStoredSettings(raw: string | null): AppSettings | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<AppSettings> | null;
    if (!parsed || typeof parsed !== "object") return null;

    const settings = defaultSettings();
    if (isLocale(parsed.locale)) {
      settings.locale = parsed.locale;
    }
    if (typeof parsed.selectedDeviceId === "string") {
      settings.selectedDeviceId = parsed.selectedDeviceId;
    }
    if (parsed.selectedDeviceId === null) {
      settings.selectedDeviceId = null;
    }
    if (parsed.devices && typeof parsed.devices === "object") {
      for (const [deviceId, value] of Object.entries(parsed.devices)) {
        if (!value || typeof value !== "object") continue;
        const stored = value as DeviceSettings;
        const sidetuneEnabled =
          typeof stored.sidetuneEnabled === "boolean"
            ? stored.sidetuneEnabled
            : undefined;
        const virtualSurroundEnabled =
          typeof stored.virtualSurroundEnabled === "boolean"
            ? stored.virtualSurroundEnabled
            : undefined;
        if (
          typeof sidetuneEnabled === "boolean" ||
          typeof virtualSurroundEnabled === "boolean"
        ) {
          settings.devices[deviceId] = {
            sidetuneEnabled,
            virtualSurroundEnabled,
          };
        }
      }
    }
    return settings;
  } catch {
    return null;
  }
}

let settings = defaultSettings();

function persistSettings() {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(APP_SETTINGS_STORAGE_KEY, JSON.stringify(settings));
  window.localStorage.setItem(LEGACY_LOCALE_STORAGE_KEY, settings.locale);
}

function loadPersistedSettings() {
  if (typeof window === "undefined") return;

  const parsed = parseStoredSettings(
    window.localStorage.getItem(APP_SETTINGS_STORAGE_KEY)
  );
  if (parsed) {
    settings = parsed;
  } else {
    const legacyLocale = window.localStorage.getItem(LEGACY_LOCALE_STORAGE_KEY);
    if (isLocale(legacyLocale)) {
      settings.locale = legacyLocale;
    }
    persistSettings();
  }

  locale.value = settings.locale;
}

function persistLocale(value: Locale) {
  if (settings.locale === value) return;
  settings.locale = value;
  persistSettings();
}

function persistSelectedDevice(deviceId: string | null) {
  if (!deviceId || settings.selectedDeviceId === deviceId) return;
  settings.selectedDeviceId = deviceId;
  persistSettings();
}

function persistedSidetuneForDevice(deviceId: string): boolean | undefined {
  return settings.devices[deviceId]?.sidetuneEnabled;
}

function persistedVirtualSurroundForDevice(deviceId: string): boolean | undefined {
  return settings.devices[deviceId]?.virtualSurroundEnabled;
}

function persistDeviceSettings(
  deviceId: string,
  updates: Partial<DeviceSettings>
) {
  const current = settings.devices[deviceId];
  const next: DeviceSettings = {
    sidetuneEnabled: current?.sidetuneEnabled,
    virtualSurroundEnabled: current?.virtualSurroundEnabled,
    ...updates,
  };

  if (
    current?.sidetuneEnabled === next.sidetuneEnabled &&
    current?.virtualSurroundEnabled === next.virtualSurroundEnabled
  ) {
    return;
  }

  settings.devices[deviceId] = next;
  persistSettings();
}

function persistSidetuneForDevice(deviceId: string, enabled: boolean) {
  persistDeviceSettings(deviceId, { sidetuneEnabled: enabled });
}

function persistVirtualSurroundForDevice(deviceId: string, enabled: boolean) {
  persistDeviceSettings(deviceId, { virtualSurroundEnabled: enabled });
}

let deviceScanTimer: number | null = null;
let devicesRequestInFlight = false;

async function loadDevices(options: { silent?: boolean } = {}) {
  if (devicesRequestInFlight) return;
  devicesRequestInFlight = true;
  const silent = options.silent ?? false;
  if (!silent) {
    devicesLoading.value = true;
  }
  deviceError.value = null;
  const previousSelection = selectedDeviceId.value;
  try {
    const result = await invoke<DeviceOption[]>("list_hyperx_devices");
    devices.value = result;

    const preferredSelection = previousSelection ?? settings.selectedDeviceId;
    const selectedStillAvailable =
      !!preferredSelection && result.some((device) => device.id === preferredSelection);
    if (selectedStillAvailable) {
      selectedDeviceId.value = preferredSelection;
    } else if (result.length > 0) {
      selectedDeviceId.value = result[0].id;
    } else {
      selectedDeviceId.value = null;
    }
    persistSelectedDevice(selectedDeviceId.value);
  } catch (error) {
    deviceError.value = describeError(error);
    console.error("Failed to load HyperX devices:", error);
    selectedDeviceId.value = null;
  } finally {
    if (!silent) {
      devicesLoading.value = false;
    }
    devicesRequestInFlight = false;
  }

  if (selectedDeviceId.value) {
    if (!silent) {
      await applyPersistedSidetonePreference(selectedDeviceId.value);
      await refreshSidetoneState();
      await applyPersistedVirtualSurroundPreference(selectedDeviceId.value);
      await refreshVirtualSurroundState();
    }
  } else {
    sidetoneError.value = null;
    virtualSurroundError.value = null;
    applySidetoneStateFromDevice(false);
    applyVirtualSurroundStateFromSystem(false);
  }
}

let suppressSidetoneWatcher = false;
let sidetoneRefreshPending = false;

function applySidetoneStateFromDevice(enabled: boolean) {
  if (sidetuneEnabled.value === enabled) {
    return;
  }
  suppressSidetoneWatcher = true;
  sidetuneEnabled.value = enabled;
}

async function applyPersistedSidetonePreference(deviceId: string) {
  const persisted = persistedSidetuneForDevice(deviceId);
  if (typeof persisted !== "boolean") return;
  await pushSidetoneState(persisted, sidetuneEnabled.value);
}

async function pushSidetoneState(enabled: boolean, fallbackState: boolean) {
  const deviceId = selectedDeviceId.value;
  if (!deviceId || devicesLoading.value) return;
  sidetoneBusy.value = true;
  sidetoneError.value = null;
  try {
    await invoke("set_sidetone", { deviceId, enabled });
    persistSelectedDevice(deviceId);
    persistSidetuneForDevice(deviceId, enabled);
  } catch (error) {
    sidetoneError.value = describeError(error);
    applySidetoneStateFromDevice(fallbackState);
  } finally {
    sidetoneBusy.value = false;
    if (sidetoneRefreshPending) {
      sidetoneRefreshPending = false;
      await refreshSidetoneState();
    }
  }
}

async function refreshSidetoneState() {
  const deviceId = selectedDeviceId.value;
  if (!deviceId || devicesLoading.value) {
    return;
  }
  if (sidetoneBusy.value) {
    sidetoneRefreshPending = true;
    return;
  }
  sidetoneBusy.value = true;
  sidetoneError.value = null;
  try {
    const state = await invoke<boolean | null>("get_sidetone_state", {
      deviceId,
    });
    if (typeof state === "boolean") {
      applySidetoneStateFromDevice(state);
      persistSelectedDevice(deviceId);
      persistSidetuneForDevice(deviceId, state);
    }
  } catch (error) {
    sidetoneError.value = describeError(error);
  } finally {
    sidetoneBusy.value = false;
    if (sidetoneRefreshPending) {
      sidetoneRefreshPending = false;
      await refreshSidetoneState();
    }
  }
}

let suppressVirtualSurroundWatcher = false;
let virtualSurroundRefreshPending = false;

function applyVirtualSurroundStateFromSystem(enabled: boolean) {
  if (virtualSurroundEnabled.value === enabled) {
    return;
  }
  suppressVirtualSurroundWatcher = true;
  virtualSurroundEnabled.value = enabled;
}

async function applyPersistedVirtualSurroundPreference(deviceId: string) {
  const persisted = persistedVirtualSurroundForDevice(deviceId);
  if (typeof persisted !== "boolean") return;
  await pushVirtualSurroundState(persisted, virtualSurroundEnabled.value);
}

async function pushVirtualSurroundState(enabled: boolean, fallbackState: boolean) {
  const deviceId =
    selectedDeviceId.value ?? settings.selectedDeviceId ?? "cloud_iii_wired";
  if (enabled && !selectedDeviceId.value) return;
  if (devicesLoading.value) return;

  virtualSurroundBusy.value = true;
  virtualSurroundError.value = null;
  try {
    await invoke("set_virtual_surround", { deviceId, enabled });
    if (selectedDeviceId.value) {
      persistSelectedDevice(deviceId);
    }
    persistVirtualSurroundForDevice(deviceId, enabled);
  } catch (error) {
    virtualSurroundError.value = describeError(error);
    applyVirtualSurroundStateFromSystem(fallbackState);
  } finally {
    virtualSurroundBusy.value = false;
    if (virtualSurroundRefreshPending) {
      virtualSurroundRefreshPending = false;
      await refreshVirtualSurroundState();
    }
  }
}

async function refreshVirtualSurroundState() {
  const deviceId = selectedDeviceId.value;
  if (!deviceId || devicesLoading.value) {
    return;
  }
  if (virtualSurroundBusy.value) {
    virtualSurroundRefreshPending = true;
    return;
  }

  virtualSurroundBusy.value = true;
  virtualSurroundError.value = null;
  try {
    const state = await invoke<boolean | null>("get_virtual_surround_state", {
      deviceId,
    });
    if (typeof state === "boolean") {
      applyVirtualSurroundStateFromSystem(state);
      persistSelectedDevice(deviceId);
      persistVirtualSurroundForDevice(deviceId, state);
    }
  } catch (error) {
    virtualSurroundError.value = describeError(error);
  } finally {
    virtualSurroundBusy.value = false;
    if (virtualSurroundRefreshPending) {
      virtualSurroundRefreshPending = false;
      await refreshVirtualSurroundState();
    }
  }
}

watch(sidetuneEnabled, async (enabled, previous) => {
  if (suppressSidetoneWatcher) {
    suppressSidetoneWatcher = false;
    return;
  }
  if (!selectedDeviceId.value || devicesLoading.value) return;
  const fallback = previous ?? !enabled;
  await pushSidetoneState(enabled, fallback);
});

watch(virtualSurroundEnabled, async (enabled, previous) => {
  if (suppressVirtualSurroundWatcher) {
    suppressVirtualSurroundWatcher = false;
    return;
  }
  if (!selectedDeviceId.value || devicesLoading.value) return;
  const fallback = previous ?? !enabled;
  await pushVirtualSurroundState(enabled, fallback);
});

watch(
  selectedDeviceId,
  async (deviceId, previous) => {
    if (deviceId === previous) return;
    if (!deviceId) {
      sidetoneError.value = null;
      virtualSurroundError.value = null;
      if (virtualSurroundEnabled.value) {
        await pushVirtualSurroundState(false, true);
      }
      applySidetoneStateFromDevice(false);
      applyVirtualSurroundStateFromSystem(false);
      return;
    }
    persistSelectedDevice(deviceId);
    if (devicesLoading.value) return;
    await applyPersistedSidetonePreference(deviceId);
    await refreshSidetoneState();
    await applyPersistedVirtualSurroundPreference(deviceId);
    await refreshVirtualSurroundState();
  },
  { immediate: true }
);
</script>

<template>
  <main
    class="min-h-screen bg-linear-to-br from-rose-100 via-orange-50 to-white text-neutral-900"
  >
    <div
      class="mx-auto flex min-h-screen w-full max-w-sm flex-col justify-start gap-10 px-6 py-14 relative"
    >
      <header class="space-y-3 text-center text-neutral-900">
        <img
          draggable="false"
          class="rounded-full w-30 h-30 mx-auto shadow-xl mb-8 hover:rotate-2 transition-transform"
          src="./assets/logo.png"
        />
        <H1 class="text-4xl font-semibold text-neutral-950 drop-shadow-none">
          {{ t("app.title") }}
        </H1>
        <p class="text-sm font-medium text-neutral-600">
          {{ t("app.subtitle") }}
        </p>
      </header>

      <section
        class="rounded-3xl bg-white/90 p-5 shadow-xl shadow-rose-200/80 ring-1 ring-black/5 backdrop-blur-md"
      >
        <div class="flex flex-col gap-3">
          <label
            class="text-xs font-semibold uppercase tracking-[0.2em] text-neutral-500"
          >
            {{ t("settings.device.label") }}
          </label>

          <Select
            v-model="deviceSelection"
            :disabled="devicesLoading || !devices.length"
          >
            <SelectTrigger
              class="h-9 bg-white/90 text-sm font-medium text-neutral-700 shadow-sm shadow-rose-200/40"
            >
              <SelectValue :placeholder="t('settings.device.placeholder')" />
            </SelectTrigger>
            <SelectContent
              align="center"
              class="w-full bg-white/95 shadow-lg shadow-rose-200/60"
            >
              <SelectItem
                v-for="device in devices"
                :key="device.id"
                :value="device.id"
              >
                {{ device.label }}
              </SelectItem>
            </SelectContent>
          </Select>

          <p v-if="deviceError" class="text-xs font-medium text-rose-600">
            {{ deviceError }}
          </p>
          <p
            v-else-if="!devicesLoading && !devices.length"
            class="text-xs font-medium text-amber-700"
          >
            {{ t("settings.device.notConnected") }}
          </p>
        </div>
      </section>

      <section
        class="rounded-3xl bg-white/90 p-6 shadow-xl shadow-rose-200/80 ring-1 ring-black/5 backdrop-blur-md"
      >
        <h2
          class="text-xs font-semibold uppercase tracking-[0.2em] text-neutral-500"
        >
          {{ t("settings.heading") }}
        </h2>

        <div class="mt-6 grid gap-6">
          <article class="relative">
            <div class="space-y-1.5">
              <h3 class="text-base font-semibold text-neutral-900">
                {{ t("settings.virtualSurround.title") }}
              </h3>
              <p class="text-xs leading-relaxed text-neutral-600">
                {{ t("settings.virtualSurround.description") }}
              </p>
            </div>

            <div class="absolute top-1 right-0">
              <Switch
                v-model="virtualSurroundEnabled"
                :aria-label="t('settings.virtualSurround.aria')"
                :disabled="!canToggleVirtualSurround"
              />
              <p
                v-if="virtualSurroundError"
                class="mt-2 max-w-[180px] text-right text-xs font-medium text-rose-600"
              >
                {{ virtualSurroundError }}
              </p>
            </div>
          </article>

          <article class="relative">
            <div class="space-y-1.5">
              <h3 class="text-base font-semibold text-neutral-900">
                {{ t("settings.sidetune.title") }}
              </h3>
              <p class="text-xs leading-relaxed text-neutral-600">
                {{ t("settings.sidetune.description") }}
              </p>
            </div>

            <div class="absolute top-1 right-0">
              <Switch
                v-model="sidetuneEnabled"
                :aria-label="t('settings.sidetune.aria')"
                :disabled="!canToggleSidetone"
              />
              <p
                v-if="sidetoneError"
                class="mt-2 max-w-[180px] text-right text-xs font-medium text-rose-600"
              >
                {{ sidetoneError }}
              </p>
            </div>
          </article>
        </div>
      </section>
      <div class="mx-auto">
        <Select v-model="selectedLocale">
          <SelectTrigger
            class="h-9 bg-white/90 text-sm font-medium text-neutral-700 shadow-sm shadow-rose-200/40"
          >
            <SelectValue :placeholder="t('settings.locale.placeholder')" />
          </SelectTrigger>
          <SelectContent
            align="end"
            class="w-36 bg-white/95 shadow-lg shadow-rose-200/60"
          >
            <SelectItem value="en">
              {{ t("settings.locale.options.en") }}
            </SelectItem>
            <SelectItem value="de">
              {{ t("settings.locale.options.de") }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  </main>
</template>
