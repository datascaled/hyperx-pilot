<script setup lang="ts">
import { cn } from "@/lib/utils"

const props = defineProps<{
  class?: string
}>()
</script>

<template>
  <h1
    :class="
      cn(
        'text-foreground text-2xl font-semibold tracking-tight drop-shadow-sm',
        props.class,
      )
    "
  >
    <slot />
  </h1>
</template>
