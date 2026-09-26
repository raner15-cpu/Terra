<script lang="ts">
    import { afterUpdate, onMount, tick } from "svelte"
    import { invoke } from "@tauri-apps/api/core"
    import {
        chatMessages,
        conversationMode,
        conversationStatus,
        lastError,
    } from "@/stores"
    import type { ChatMessage } from "@/lib/ipc"

    let models: string[] = []
    let selectedModel = ""
    let input = ""
    let busy = false
    let status = ""
    let errorMessage = ""
    let conversation: HTMLDivElement | undefined

    $: canSend = Boolean(selectedModel && input.trim() && !busy)
    $: voiceStatus = {
        idle: "Голосовой разговор не активен",
        listening: "Слушаю…",
        recognizing: "Распознаю речь…",
        transcribing: "Расшифровываю через Whisper…",
        thinking: "Думаю…",
        answering: "Отвечаю…",
        error: "Ошибка разговора",
    }[$conversationStatus]

    afterUpdate(() => {
        if (conversation) {
            conversation.scrollTop = conversation.scrollHeight
        }
    })

    onMount(async () => {
        try {
            selectedModel = await invoke<string>("db_read", { key: "local_llm_model" })
        } catch {
            selectedModel = ""
        }
        await loadModels()
    })

    async function loadModels() {
        status = "Подключаюсь к локальному Ollama…"
        errorMessage = ""

        try {
            models = await invoke<string[]>("list_local_llm_models")
            if (models.length === 0) {
                status = "Ollama доступна, но локальные модели не найдены."
                selectedModel = ""
                return
            }

            if (!models.includes(selectedModel)) {
                selectedModel = models[0]
            }
            await saveSelectedModel()
            status = "Диалог работает локально. История хранится только в памяти этой страницы."
        } catch (error) {
            models = []
            selectedModel = ""
            status = "Для диалога нужна запущенная Ollama с установленной локальной моделью."
            errorMessage = String(error)
        }
    }

    async function saveSelectedModel() {
        if (!selectedModel) return
        await invoke<boolean>("db_write", {
            key: "local_llm_model",
            val: selectedModel,
        })
    }

    async function submitMessage(event: Event) {
        event.preventDefault()
        const text = input.trim()
        if (!text || !selectedModel || busy) return

        const previousMessages = $chatMessages
        const nextMessages: ChatMessage[] = [
            ...previousMessages,
            { role: "user", content: text },
        ]
        $chatMessages = nextMessages
        input = ""
        busy = true
        errorMessage = ""

        try {
            const answer = await invoke<string>("chat_local", {
                model: selectedModel,
                messages: nextMessages,
            })
            $chatMessages = [...nextMessages, { role: "assistant", content: answer }]
            await tick()
            if (conversation) conversation.scrollTop = conversation.scrollHeight
        } catch (error) {
            $chatMessages = previousMessages
            input = text
            errorMessage = String(error)
        } finally {
            busy = false
        }
    }

    function clearConversation() {
        $chatMessages = []
        errorMessage = ""
    }
</script>

<section class="terra-chat">
    <header class="chat-heading">
        <div>
            <p class="eyebrow">TERRA · ЛОКАЛЬНЫЙ РЕЖИМ</p>
            <h1>Текстовый диалог</h1>
            <p class="intro">
                Это первый скелет чата: Терра отвечает через модель на твоём компьютере.
                Пока она не открывает файлы и не выполняет команды.
            </p>
        </div>
        <button class="clear-button" type="button" on:click={clearConversation} disabled={$chatMessages.length === 0 || busy || $conversationMode}>
            Новый диалог
        </button>
    </header>

    <div class="model-bar">
        <label for="local-model">Локальная модель</label>
        <select id="local-model" bind:value={selectedModel} on:change={saveSelectedModel} disabled={models.length === 0 || busy || $conversationMode}>
            {#each models as model}
                <option value={model}>{model}</option>
            {/each}
        </select>
        <button class="refresh-button" type="button" on:click={loadModels} disabled={busy}>
            Обновить
        </button>
    </div>

    <p class="connection-status" class:voice-active={$conversationMode}>
        {$conversationMode
            ? `Голосовой разговор · ${voiceStatus} · скажи «Закончи разговор», чтобы выйти`
            : status}
    </p>

    <div class="messages" bind:this={conversation} aria-live="polite">
        {#if $chatMessages.length === 0}
            <div class="empty-state">
                <div class="pulse">T</div>
                <h2>Начни с любого вопроса</h2>
                <p>Ничего из этого диалога не сохраняется между запусками.</p>
            </div>
        {/if}

        {#each $chatMessages as message}
            <article class="message {message.role}">
                <span class="speaker">{message.role === "user" ? "Ты" : "Терра"}</span>
                <p class:thinking={!message.content}>
                    {message.content || "Начинаю отвечать…"}
                </p>
            </article>
        {/each}

        {#if busy || ($conversationMode && $conversationStatus === "thinking")}
            <article class="message assistant">
                <span class="speaker">Терра</span>
                <p class="thinking">Думаю…</p>
            </article>
        {/if}
    </div>

    {#if errorMessage}
        <p class="error-message" role="alert">{errorMessage}</p>
    {/if}
    {#if $conversationMode && $lastError}
        <p class="error-message" role="alert">{$lastError}</p>
    {/if}

    <form class="composer" on:submit={submitMessage}>
        <textarea
            bind:value={input}
            placeholder={models.length ? "Напиши Терре…" : "Сначала запусти Ollama и установи модель"}
            rows="2"
            maxlength="12000"
            disabled={!selectedModel || busy || $conversationMode}
            on:keydown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault()
                    if (canSend) submitMessage(event)
                }
            }}
        ></textarea>
        <button type="submit" disabled={!canSend || $conversationMode}>{busy ? "…" : "Отправить"}</button>
        <small>Enter — отправить · Shift+Enter — новая строка</small>
    </form>
</section>

<style lang="scss">
    .terra-chat {
        --terra-blue: #3296ff;
        --terra-blue-soft: rgba(50, 150, 255, 0.14);
        max-width: 880px;
        height: calc(100vh - 115px);
        min-height: 500px;
        margin: 0 auto;
        padding: 1.25rem 1rem 1rem;
        display: flex;
        flex-direction: column;
        color: #e8f1fc;
    }

    .chat-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
    }

    .eyebrow {
        margin: 0 0 0.35rem;
        color: var(--terra-blue);
        font-size: 0.68rem;
        font-weight: 700;
        letter-spacing: 0.16em;
    }

    h1 {
        margin: 0;
        font-size: 1.5rem;
        font-weight: 600;
    }

    .intro {
        max-width: 620px;
        margin: 0.5rem 0 0;
        color: #aebed1;
        font-size: 0.88rem;
        line-height: 1.45;
    }

    button, select, textarea {
        font: inherit;
    }

    button {
        border: 1px solid rgba(50, 150, 255, 0.35);
        border-radius: 8px;
        color: #e8f1fc;
        background: rgba(28, 48, 72, 0.85);
        cursor: pointer;
        transition: background 0.15s ease, border-color 0.15s ease;
    }

    button:hover:not(:disabled) {
        border-color: var(--terra-blue);
        background: rgba(50, 150, 255, 0.2);
    }

    button:disabled {
        opacity: 0.45;
        cursor: not-allowed;
    }

    .clear-button, .refresh-button {
        padding: 0.55rem 0.8rem;
        font-size: 0.78rem;
        white-space: nowrap;
    }

    .model-bar {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        margin-top: 1.1rem;
    }

    .model-bar label {
        color: #aebed1;
        font-size: 0.78rem;
    }

    select {
        min-width: 0;
        flex: 1;
        padding: 0.55rem 0.65rem;
        border: 1px solid rgba(50, 150, 255, 0.3);
        border-radius: 7px;
        color: #e8f1fc;
        background: #111c2a;
    }

    .connection-status {
        min-height: 1.25rem;
        margin: 0.45rem 0;
        color: #92a8c0;
        font-size: 0.72rem;
    }

    .messages {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 0.5rem 0.25rem;
        border-top: 1px solid rgba(142, 178, 220, 0.12);
        border-bottom: 1px solid rgba(142, 178, 220, 0.12);
    }

    .empty-state {
        min-height: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        color: #8fa3ba;
        text-align: center;
    }

    .empty-state h2 {
        margin: 0.9rem 0 0.35rem;
        color: #dce9f8;
        font-size: 1rem;
        font-weight: 500;
    }

    .empty-state p {
        margin: 0;
        font-size: 0.78rem;
    }

    .pulse {
        width: 54px;
        height: 54px;
        display: grid;
        place-items: center;
        border: 1px solid rgba(50, 150, 255, 0.65);
        border-radius: 50%;
        color: #d9ebff;
        background: var(--terra-blue-soft);
        box-shadow: 0 0 28px rgba(50, 150, 255, 0.22);
        font-size: 1.25rem;
    }

    .message {
        max-width: 88%;
        margin: 0.8rem 0;
        padding: 0.75rem 0.9rem;
        border: 1px solid rgba(142, 178, 220, 0.12);
        border-radius: 10px;
        background: rgba(17, 28, 42, 0.82);
    }

    .message.user {
        margin-left: auto;
        border-color: rgba(50, 150, 255, 0.25);
        background: rgba(26, 55, 88, 0.45);
    }

    .speaker {
        display: block;
        margin-bottom: 0.3rem;
        color: var(--terra-blue);
        font-size: 0.7rem;
        font-weight: 700;
    }

    .message p {
        margin: 0;
        color: #e5edf7;
        font-size: 0.9rem;
        line-height: 1.55;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }

    .message .thinking {
        color: #9eb4cd;
    }

    .error-message {
        margin: 0.5rem 0 0;
        color: #ffaaa7;
        font-size: 0.78rem;
        overflow-wrap: anywhere;
    }

    .composer {
        position: relative;
        display: grid;
        grid-template-columns: 1fr auto;
        gap: 0.55rem;
        padding-top: 0.75rem;
    }

    textarea {
        min-height: 58px;
        resize: vertical;
        padding: 0.75rem;
        border: 1px solid rgba(50, 150, 255, 0.35);
        border-radius: 9px;
        outline: none;
        color: #e8f1fc;
        background: #101a27;
    }

    textarea:focus {
        border-color: var(--terra-blue);
        box-shadow: 0 0 0 2px rgba(50, 150, 255, 0.12);
    }

    .composer > button {
        align-self: stretch;
        min-width: 96px;
        padding: 0 0.9rem;
        background: rgba(50, 150, 255, 0.22);
    }

    .composer small {
        grid-column: 1 / -1;
        color: #788da5;
        font-size: 0.68rem;
    }

    @media (max-width: 600px) {
        .terra-chat {
            height: calc(100vh - 90px);
            padding: 0.8rem 0.65rem;
        }
        .chat-heading {
            flex-direction: column;
        }
        .clear-button {
            align-self: flex-end;
        }
        .model-bar {
            flex-wrap: wrap;
        }
    }
</style>