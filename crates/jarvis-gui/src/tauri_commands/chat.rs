use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";
const TERRA_SYSTEM_PROMPT: &str = "Ты — Терра, локальный текстовый помощник пользователя. \
Отвечай по-русски, если пользователь не попросил другой язык. Будь ясной, доброжелательной \
и честно говори о своих ограничениях. В этой версии у тебя нет доступа к файлам, браузеру, \
микрофону или системным командам: не утверждай, что выполнила такие действия.";
const MAX_MESSAGES: usize = 40;
const MAX_TOTAL_CHARACTERS: usize = 48_000;

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaReply,
}

#[derive(Deserialize)]
struct OllamaReply {
    content: String,
}

fn ollama_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| format!("Не удалось создать клиент локальной модели: {error}"))
}

async fn fetch_local_models(client: &Client) -> Result<Vec<String>, String> {
    let response = client
        .get(format!("{OLLAMA_BASE_URL}/api/tags"))
        .send()
        .await
        .map_err(|error| {
            format!(
                "Не удалось подключиться к Ollama на 127.0.0.1:11434. Запусти Ollama и установи локальную модель. ({error})"
            )
        })?;

    if !response.status().is_success() {
        return Err(format!(
            "Локальный сервер моделей вернул HTTP {} при получении списка моделей.",
            response.status()
        ));
    }

    let payload = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|error| format!("Не удалось прочитать список локальных моделей: {error}"))?;

    let mut models: Vec<String> = payload.models.into_iter().map(|model| model.name).collect();
    models.sort();
    models.dedup();
    Ok(models)
}

#[tauri::command]
pub async fn list_local_llm_models() -> Result<Vec<String>, String> {
    fetch_local_models(&ollama_client()?).await
}

#[tauri::command]
pub async fn chat_local(model: String, messages: Vec<ChatMessage>) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Сначала выбери установленную локальную модель.".to_string());
    }
    if messages.is_empty() {
        return Err("В диалоге пока нет сообщения.".to_string());
    }
    if messages.len() > MAX_MESSAGES {
        return Err("Диалог стал слишком длинным. Очисти его и начни новый.".to_string());
    }

    let mut total_characters = 0;
    for message in &messages {
        if message.role != "user" && message.role != "assistant" {
            return Err("В диалоге обнаружена неподдерживаемая роль сообщения.".to_string());
        }
        total_characters += message.content.chars().count();
        if total_characters > MAX_TOTAL_CHARACTERS {
            return Err("Диалог стал слишком длинным. Очисти его и начни новый.".to_string());
        }
    }

    let client = ollama_client()?;
    let available_models = fetch_local_models(&client).await?;
    if !available_models
        .iter()
        .any(|available| available.as_str() == model)
    {
        return Err("Выбранная модель не найдена в Ollama. Обнови список моделей.".to_string());
    }

    let mut request_messages = Vec::with_capacity(messages.len() + 1);
    request_messages.push(OllamaMessage {
        role: "system",
        content: TERRA_SYSTEM_PROMPT,
    });
    request_messages.extend(messages.iter().map(|message| OllamaMessage {
        role: &message.role,
        content: &message.content,
    }));

    let response = client
        .post(format!("{OLLAMA_BASE_URL}/api/chat"))
        .json(&OllamaChatRequest {
            model,
            messages: request_messages,
            stream: false,
        })
        .send()
        .await
        .map_err(|error| format!("Ошибка запроса к локальной модели: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let details = response.text().await.unwrap_or_default();
        return Err(format!("Локальная модель вернула HTTP {status}. {details}"));
    }

    let payload = response
        .json::<OllamaChatResponse>()
        .await
        .map_err(|error| format!("Не удалось прочитать ответ локальной модели: {error}"))?;

    let answer = payload.message.content.trim().to_string();
    if answer.is_empty() {
        return Err("Локальная модель вернула пустой ответ.".to_string());
    }

    Ok(answer)
}
