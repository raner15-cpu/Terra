<script lang="ts">
    import { onMount, onDestroy } from "svelte"
    import { Router } from "@roxi/routify"
    import routes from "../.routify/routes.default.js"
    import { SvelteUIProvider } from "@svelteuidev/core"
    import Events from "./Events.svelte"

    import {
        loadVoiceSetting,
        loadAppInfo,
        startStatsPolling,
        stopStatsPolling,
        enableIpc,
        disableIpc,
        loadTranslations
    } from "@/stores"

    onMount(() => {
        // load static data
        loadVoiceSetting()
        loadAppInfo()

        // start process monitoring
        startStatsPolling(5000)

        // Keep one IPC connection alive for the whole application.
        // Route components must not own or close this connection.
        enableIpc()

        // load language
        loadTranslations()
    })

    onDestroy(() => {
        stopStatsPolling()
        disableIpc()
    })
</script>

<SvelteUIProvider themeObserver="dark" withNormalizeCSS withGlobalStyles>
    <Router {routes} />
</SvelteUIProvider>

<Events />
