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

const LOCALE_STORAGE_KEY = "hyperx:locale";
const DEVICE_SCAN_INTERVAL_MS = 3000;

interface DeviceOption {
  id: string;
  label: string;
}

const { t, locale } = useI18n();

const sidetuneEnabled = ref(false);
const devices = ref<DeviceOption[]>([]);
const devicesLoading = ref(true);
const deviceError = ref<string | null>(null);
const selectedDeviceId = ref<string | null>(null);
const sidetoneBusy = ref(false);
const sidetoneError = ref<string | null>(null);

const deviceSelection = computed<string>({
  get: () => selectedDeviceId.value ?? "",
  set: (value) => {
    selectedDeviceId.value = value || null;
  },
});

const canToggleSidetone = computed(
  () => !!selectedDeviceId.value && !devicesLoading.value && !sidetoneBusy.value
);

const selectedLocale = computed<Locale>({
  get: () => locale.value as Locale,
  set: (value) => {
    locale.value = value;
    if (typeof window !== "undefined") {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, value);
    }
  },
});

onMounted(() => {
  void loadDevices();
  if (typeof window === "undefined") return;
  const stored = window.localStorage.getItem(
    LOCALE_STORAGE_KEY
  ) as Locale | null;
  if (stored && stored in messages) {
    locale.value = stored;
  }
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

let deviceScanTimer: number | null = null;
let devicesRequestInFlight = false;

async function loadDevices(options: { silent?: boolean } = {}) {
  if (devicesRequestInFlight) return;
  devicesRequestInFlight = true;
  const silent = options.silent ?? false;
  let selectionChanged = false;
  if (!silent) {
    devicesLoading.value = true;
  }
  deviceError.value = null;
  const previousSelection = selectedDeviceId.value;
  try {
    const result = await invoke<DeviceOption[]>("list_hyperx_devices");
    devices.value = result;
    const selectedStillAvailable =
      !!previousSelection && result.some((device) => device.id === previousSelection);
    if (selectedStillAvailable) {
      selectedDeviceId.value = previousSelection;
    } else if (result.length > 0) {
      selectedDeviceId.value = result[0].id;
    } else {
      selectedDeviceId.value = null;
    }
    selectionChanged = selectedDeviceId.value !== previousSelection;
  } catch (error) {
    deviceError.value = describeError(error);
    console.error("Failed to load HyperX devices:", error);
    selectedDeviceId.value = null;
    selectionChanged = selectedDeviceId.value !== previousSelection;
  } finally {
    if (!silent) {
      devicesLoading.value = false;
    }
    devicesRequestInFlight = false;
  }

  if (selectedDeviceId.value) {
    if (!silent || selectionChanged) {
      await refreshSidetoneState();
    }
  } else {
    sidetoneError.value = null;
    applySidetoneStateFromDevice(false);
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

async function pushSidetoneState(enabled: boolean, fallbackState: boolean) {
  if (!selectedDeviceId.value || devicesLoading.value) return;
  sidetoneBusy.value = true;
  sidetoneError.value = null;
  try {
    await invoke("set_sidetone", { deviceId: selectedDeviceId.value, enabled });
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
  if (!selectedDeviceId.value || devicesLoading.value) {
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
      deviceId: selectedDeviceId.value,
    });
    if (typeof state === "boolean") {
      applySidetoneStateFromDevice(state);
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

watch(sidetuneEnabled, async (enabled, previous) => {
  if (suppressSidetoneWatcher) {
    suppressSidetoneWatcher = false;
    return;
  }
  if (!selectedDeviceId.value || devicesLoading.value) return;
  const fallback = previous ?? !enabled;
  await pushSidetoneState(enabled, fallback);
});

watch(
  selectedDeviceId,
  async (deviceId, previous) => {
    if (deviceId === previous) return;
    if (!deviceId) {
      sidetoneError.value = null;
      applySidetoneStateFromDevice(false);
      return;
    }
    if (devicesLoading.value) return;
    await refreshSidetoneState();
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
