<script lang="ts">
  import "../app.css";

  let { children } = $props();
</script>

<div class="flex h-screen w-full overflow-hidden">
  {@render children()}
</div>
