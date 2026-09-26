use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";
const SYSTEM_PROMPT: &str = "Ты — Терра, локальный голосовой помощник пользователя. \
Отвечай по-русски, если пользователь не попросил другой язык. Отвечай естественно и по существу. \
Ты находишься в режиме разговора и не выполняешь системные команды: действия на компьютере \
обрабатываются отдельным безопасным маршрутизатором Terra.";
const MAX_MESSAGES: usize = 40;
const MAX_TOTAL_CHARACTERS: usize = 48_000;

#[derive(Clone, Debug, Serialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug)]
pub struct ConversationReply {
    pub text: String,
    pub model: String,
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

#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct OllamaMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OllamaReply {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct OllamaStreamResponse {
    #[serde(default)]
    message: Option<OllamaReply>,
    #[serde(default)]
    done: bool,
    error: Option<String>,
}

fn client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|error| format!("Не удалось создать клиент Ollama: {error}"))
}

async fn available_models(client: &Client) -> Result<Vec<String>, String> {
    let response = client
        .get(format!("{OLLAMA_BASE_URL}/api/tags"))
        .send()
        .await
        .map_err(|error| {
            format!(
                "Не удалось подключиться к Ollama на 127.0.0.1:11434. \
Запусти Ollama и установи локальную модель. ({error})"
            )
        })?;

    if !response.status().is_success() {
        return Err(format!(
            "Ollama вернула HTTP {} при получении списка моделей.",
            response.status()
        ));
    }

    let payload = response
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|error| format!("Не удалось прочитать список моделей Ollama: {error}"))?;

    let mut models: Vec<String> = payload.models.into_iter().map(|model| model.name).collect();
    models.sort();
    models.dedup();
    Ok(models)
}

pub async fn chat_stream<F>(
    preferred_model: Option<&str>,
    history: &[ConversationMessage],
    mut on_chunk: F,
) -> Result<ConversationReply, String>
where
    F: FnMut(&str),
{
    if history.is_empty() {
        return Err("В голосовом диалоге пока нет сообщения.".to_string());
    }
    if history.len() > MAX_MESSAGES {
        return Err("Голосовой диалог стал слишком длинным. Начни новый разговор.".to_string());
    }

    let total_characters: usize = history
        .iter()
        .map(|message| message.content.chars().count())
        .sum();
    if total_characters > MAX_TOTAL_CHARACTERS {
        return Err("Голосовой диалог стал слишком длинным. Начни новый разговор.".to_string());
    }

    let client = client()?;
    let models = available_models(&client).await?;
    if models.is_empty() {
        return Err("Ollama доступна, но локальные модели не найдены.".to_string());
    }

    let preferred = preferred_model.unwrap_or_default().trim();
    let model = if !preferred.is_empty() && models.iter().any(|model| model == preferred) {
        preferred.to_string()
    } else {
        models[0].clone()
    };

    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(OllamaMessage {
        role: "system",
        content: SYSTEM_PROMPT,
    });
    messages.extend(history.iter().map(|message| OllamaMessage {
        role: &message.role,
        content: &message.content,
    }));

    let response = client
        .post(format!("{OLLAMA_BASE_URL}/api/chat"))
        .json(&OllamaChatRequest {
            model: &model,
            messages,
            stream: true,
        })
        .send()
        .await
        .map_err(|error| format!("Ошибка запроса к локальной модели: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        let details = response.text().await.unwrap_or_default();
        return Err(format!("Ollama вернула HTTP {status}. {details}"));
    }

    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut answer = String::new();

    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|error| format!("Ошибка потока Ollama: {error}"))?;
        pending.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(newline) = pending.find('\n') {
            let line = pending[..newline].trim().to_string();
            pending.drain(..=newline);
            if line.is_empty() {
                continue;
            }

            let payload: OllamaStreamResponse = serde_json::from_str(&line)
                .map_err(|error| format!("Не удалось прочитать поток Ollama: {error}"))?;
            if let Some(error) = payload.error {
                return Err(format!("Ollama вернула ошибку: {error}"));
            }
            if let Some(message) = payload.message {
                if !message.content.is_empty() {
                    on_chunk(&message.content);
                    answer.push_str(&message.content);
                }
            }
            if payload.done {
                break;
            }
        }
    }

    if !pending.trim().is_empty() {
        let payload: OllamaStreamResponse = serde_json::from_str(pending.trim())
            .map_err(|error| format!("Не удалось прочитать завершение потока Ollama: {error}"))?;
        if let Some(error) = payload.error {
            return Err(format!("Ollama вернула ошибку: {error}"));
        }
        if let Some(message) = payload.message {
            if !message.content.is_empty() {
                on_chunk(&message.content);
                answer.push_str(&message.content);
            }
        }
    }

    let text = answer.trim().to_string();
    if text.is_empty() {
        return Err("Локальная модель вернула пустой ответ.".to_string());
    }

    Ok(ConversationReply { text, model })
}