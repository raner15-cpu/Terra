<script lang="ts">
    import { onMount, onDestroy } from "svelte"
    import { invoke } from "@tauri-apps/api/core"

    import SearchBar from "@/components/elements/SearchBar.svelte"
    import ArcReactor from "@/components/elements/ArcReactor.svelte"
    import HDivider from "@/components/elements/HDivider.svelte"
    import Stats from "@/components/elements/Stats.svelte"

    import {
        isJarvisRunning,
        updateJarvisStats,
        enableIpc,
        disableIpc,
        translate,
        translations
    } from "@/stores"

    $: t = (key: string) => translate($translations, key)

    let processRunning = false
    let launching = false
    let assistantError = ""
    let wasRunning = false

    isJarvisRunning.subscribe((value) => {
        processRunning = value
        if (value) {
            enableIpc()
            wasRunning = true
        } else if (wasRunning) {
            disableIpc()
            wasRunning = false
        }
    })

    onMount(() => {
        updateJarvisStats()
    })

    onDestroy(() => {
        disableIpc()
    })

    async function runAssistant() {
        launching = true
        assistantError = ""
        try {
            await invoke("run_jarvis_app")
            setTimeout(async () => {
                await updateJarvisStats()
                launching = false
            }, 2500)
        } catch (err) {
            console.error("Failed to run Terra voice assistant:", err)
            assistantError = String(err)
            launching = false
        }
    }
</script>

<div class="app-container assist-page">
    <div class="search search-section">
        <HDivider />
        <SearchBar />
    </div>

    <div class="reactor-section">
        <div class="reactor-wrapper" class:dimmed={!processRunning}>
            <ArcReactor />
        </div>

        {#if !processRunning}
            <div class="offline-badge">
                <span class="offline-icon">⚠</span>
                <span class="offline-text">{t('assistant-not-running')}</span>
                <small>{t('assistant-offline-hint')}</small>
            </div>
            <button
                class="start-button"
                on:click={runAssistant}
                disabled={launching}
            >
                {launching ? t('btn-starting') : t('btn-start')}
            </button>
            {#if assistantError}
                <p class="assistant-error" role="alert">{assistantError}</p>
            {/if}
        {/if}
    </div>

    <HDivider noMargin />
    <Stats />
</div>

<style>
    .assistant-error {
        max-width: 620px;
        margin: 0.75rem auto 0;
        color: #ff8f8f;
        font-size: 0.8rem;
        line-height: 1.45;
        overflow-wrap: anywhere;
        text-align: center;
    }
</style>
