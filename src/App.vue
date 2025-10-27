<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import H1 from "@/components/H1.vue";
import { Switch } from "@/components/ui/switch";

const { t } = useI18n();

const dtsEnabled = ref(false);
const sidetuneEnabled = ref(false);
</script>

<template>
  <main
    class="min-h-screen bg-linear-to-br from-rose-100 via-orange-50 to-white text-neutral-900"
  >
    <div
      class="mx-auto flex min-h-screen w-full max-w-sm flex-col justify-start gap-10 px-6 py-14"
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
        class="rounded-3xl bg-white/90 p-6 shadow-xl shadow-rose-200/80 ring-1 ring-black/5 backdrop-blur-md"
      >
        <h2
          class="text-xs font-semibold uppercase tracking-[0.2em] text-neutral-500"
        >
          {{ t("settings.heading") }}
        </h2>

        <div class="mt-6 grid gap-6">
          <article
            class="flex items-start justify-between gap-4 rounded-2xl border border-black/5 bg-white p-4 shadow-sm shadow-rose-200/40"
          >
            <div class="space-y-1.5">
              <h3 class="text-base font-semibold text-neutral-900">
                {{ t("settings.dts.title") }}
              </h3>
              <p class="text-sm leading-relaxed text-neutral-600">
                {{ t("settings.dts.description") }}
              </p>
            </div>

            <div class="flex flex-col items-end mt-1">
              <Switch
                v-model="dtsEnabled"
                :aria-label="t('settings.dts.aria')"
              />
            </div>
          </article>

          <article
            class="flex items-start justify-between gap-4 rounded-2xl border border-black/5 bg-white p-4 shadow-sm shadow-rose-200/40"
          >
            <div class="space-y-1.5">
              <h3 class="text-base font-semibold text-neutral-900">
                {{ t("settings.sidetune.title") }}
              </h3>
              <p class="text-sm leading-relaxed text-neutral-600">
                {{ t("settings.sidetune.description") }}
              </p>
            </div>

            <div class="flex flex-col items-end mt-1">
              <Switch v-model="sidetuneEnabled" />
            </div>
          </article>
        </div>
      </section>
    </div>
  </main>
</template>
