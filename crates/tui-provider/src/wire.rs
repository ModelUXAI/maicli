//! Konversi pesan ke bentuk wire OpenAI-compatible dan perakitan ulang
//! (`tool_call` terfragmentasi, teks, usage, finish_reason).

use serde::Deserialize;
use tui_wire::{
    AssistantMessage, ContentBlock, Message, ProviderEvent, StopReason, ToolDefinition, Usage,
};

/// Bentuk pesan untuk gateway.
pub fn to_wire_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages.iter().map(message_to_wire).collect()
}

fn message_to_wire(message: &Message) -> serde_json::Value {
    match message {
        Message::System { content } => {
            serde_json::json!({ "role": "system", "content": content })
        }
        Message::User { content } => serde_json::json!({ "role": "user", "content": content }),
        Message::Assistant { content, .. } => {
            let mut text = String::new();
            let mut calls = Vec::new();
            for block in content {
                match block {
                    ContentBlock::Text { text: t } => text.push_str(t),
                    ContentBlock::Thinking { .. } => {}
                    ContentBlock::ToolUse {
                        id,
                        name,
                        arguments,
                    } => calls.push(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": arguments }
                    })),
                }
            }
            if calls.is_empty() {
                serde_json::json!({ "role": "assistant", "content": text })
            } else {
                let content_value = if text.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(text)
                };
                serde_json::json!({
                    "role": "assistant",
                    "content": content_value,
                    "tool_calls": calls
                })
            }
        }
        Message::ToolResult {
            tool_call_id,
            content,
            ..
        } => serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": content
        }),
    }
}

/// Definisi tool dalam bentuk yang dipakai gateway.
pub fn to_wire_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                }
            })
        })
        .collect()
}

#[derive(Debug, Default, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Debug, Default, Deserialize)]
struct ToolCallDelta {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Debug, Default, Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: Option<PromptDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct PromptDetails {
    #[serde(default)]
    cached_tokens: u64,
}

#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

/// Perakit satu respons streaming menjadi `AssistantMessage`.
#[derive(Debug, Default)]
pub struct StreamAssembler {
    text: String,
    thinking: String,
    text_started: bool,
    thinking_started: bool,
    calls: Vec<PartialCall>,
    finish_reason: Option<String>,
    usage: Usage,
}

impl StreamAssembler {
    /// Rakit payload JSON satu chunk menjadi event provider.
    pub fn push(&mut self, json: &str) -> Result<Vec<ProviderEvent>, serde_json::Error> {
        let chunk: StreamChunk = serde_json::from_str(json)?;
        let mut events = Vec::new();
        if let Some(usage) = chunk.usage {
            self.usage = Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
                cached_tokens: usage.prompt_tokens_details.map_or(0, |d| d.cached_tokens),
            };
        }
        let Some(choice) = chunk.choices.first() else {
            return Ok(events);
        };

        if let Some(text) = choice.delta.content.as_deref().filter(|t| !t.is_empty()) {
            if !self.text_started {
                self.text_started = true;
                events.push(ProviderEvent::TextStart);
            }
            self.text.push_str(text);
            events.push(ProviderEvent::TextDelta {
                text: text.to_string(),
            });
        }
        if let Some(text) = choice
            .delta
            .reasoning_content
            .as_deref()
            .filter(|t| !t.is_empty())
        {
            if !self.thinking_started {
                self.thinking_started = true;
                events.push(ProviderEvent::ThinkingStart);
            }
            self.thinking.push_str(text);
            events.push(ProviderEvent::ThinkingDelta {
                text: text.to_string(),
            });
        }
        for delta in &choice.delta.tool_calls {
            let index = delta.index.unwrap_or(0);
            let slot = index as usize;
            while self.calls.len() <= slot {
                self.calls.push(PartialCall::default());
            }
            let Some(call) = self.calls.get_mut(slot) else {
                continue;
            };
            if let Some(id) = delta.id.as_deref() {
                call.id.push_str(id);
            }
            if let Some(function) = &delta.function {
                if let Some(name) = function.name.as_deref() {
                    call.name.push_str(name);
                }
                if !call.started {
                    call.started = true;
                    events.push(ProviderEvent::ToolCallStart {
                        index,
                        id: call.id.clone(),
                        name: call.name.clone(),
                    });
                }
                if let Some(arguments) = function.arguments.as_deref().filter(|a| !a.is_empty()) {
                    call.arguments.push_str(arguments);
                    events.push(ProviderEvent::ToolCallDelta {
                        index,
                        arguments: arguments.to_string(),
                    });
                }
            } else if !call.started && !call.name.is_empty() {
                call.started = true;
                events.push(ProviderEvent::ToolCallStart {
                    index,
                    id: call.id.clone(),
                    name: call.name.clone(),
                });
            }
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            self.finish_reason = Some(reason.to_string());
        }
        Ok(events)
    }

    /// Tutup stream: event penutup blok lalu `Done` dengan pesan akhir.
    pub fn finish(self) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        if self.text_started {
            events.push(ProviderEvent::TextEnd);
        }
        if self.thinking_started {
            events.push(ProviderEvent::ThinkingEnd);
        }
        for (index, _) in self.calls.iter().enumerate() {
            events.push(ProviderEvent::ToolCallEnd {
                index: index as u32,
            });
        }
        let message = self.into_message();
        events.push(ProviderEvent::Done { message });
        events
    }

    fn into_message(self) -> AssistantMessage {
        let mut content = Vec::new();
        if !self.thinking.is_empty() {
            content.push(ContentBlock::Thinking {
                text: self.thinking,
            });
        }
        if !self.text.is_empty() {
            content.push(ContentBlock::Text { text: self.text });
        }
        for call in self.calls {
            content.push(ContentBlock::ToolUse {
                id: call.id,
                name: call.name,
                arguments: call.arguments,
            });
        }
        let stop_reason = match self.finish_reason.as_deref() {
            Some("tool_calls" | "function_call") => StopReason::ToolUse,
            Some("length") => StopReason::Length,
            Some("content_filter") => StopReason::Error,
            _ => StopReason::Stop,
        };
        let error = if matches!(stop_reason, StopReason::Error) {
            Some("gateway menolak konten (content_filter)".to_string())
        } else {
            None
        };
        AssistantMessage {
            content,
            stop_reason,
            usage: self.usage,
            error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_terfragmentasi_dirakit() {
        let mut asm = StreamAssembler::default();
        let first = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":"{\"pa"}}]},"finish_reason":null}]}"#;
        let second = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a"}}]},"finish_reason":null}]}"#;
        let third = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":".txt\"}"}}]},"finish_reason":"tool_calls"}]}"#;
        assert!(!asm.push(first).unwrap().is_empty());
        asm.push(second).unwrap();
        asm.push(third).unwrap();
        let events = asm.finish();
        let done = events.last().unwrap();
        match done {
            ProviderEvent::Done { message } => {
                assert_eq!(message.stop_reason, StopReason::ToolUse);
                let calls = message.tool_calls();
                assert_eq!(calls.len(), 1);
                let (id, name, arguments) = calls[0];
                assert_eq!(id, "call_1");
                assert_eq!(name, "read");
                assert_eq!(arguments, "{\"path\":\"a.txt\"}");
            }
            other => panic!("event terakhir harus Done, dapat {other:?}"),
        }
    }

    #[test]
    fn teks_dan_finish_stop() {
        let mut asm = StreamAssembler::default();
        asm.push(r#"{"choices":[{"delta":{"content":"halo"},"finish_reason":null}]}"#)
            .unwrap();
        asm.push(r#"{"choices":[{"delta":{"content":" dunia"},"finish_reason":"stop"}]}"#)
            .unwrap();
        let events = asm.finish();
        match events.last().unwrap() {
            ProviderEvent::Done { message } => {
                assert_eq!(message.stop_reason, StopReason::Stop);
                assert_eq!(message.joined_text(), "halo dunia");
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn length_dipetakan() {
        let mut asm = StreamAssembler::default();
        asm.push(r#"{"choices":[{"delta":{},"finish_reason":"length"}]}"#)
            .unwrap();
        match asm.finish().last().unwrap() {
            ProviderEvent::Done { message } => assert_eq!(message.stop_reason, StopReason::Length),
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn usage_terbaca_termasuk_cached() {
        let mut asm = StreamAssembler::default();
        asm.push(r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":7}}}"#)
            .unwrap();
        match asm.finish().last().unwrap() {
            ProviderEvent::Done { message } => {
                assert_eq!(message.usage.input_tokens, 10);
                assert_eq!(message.usage.output_tokens, 3);
                assert_eq!(message.usage.cached_tokens, 7);
            }
            other => panic!("dapat {other:?}"),
        }
    }

    #[test]
    fn pesan_ke_wire() {
        let messages = vec![
            Message::system("aturan"),
            Message::user("baca a.txt"),
            Message::tool_result("call_1", "read", "isi", false, false),
        ];
        let wire = to_wire_messages(&messages);
        assert_eq!(wire[0]["role"], "system");
        assert_eq!(wire[1]["content"], "baca a.txt");
        assert_eq!(wire[2]["role"], "tool");
        assert_eq!(wire[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn assistant_dengan_tool_call_di_wire() {
        let message = Message::assistant(&AssistantMessage {
            content: vec![ContentBlock::ToolUse {
                id: "c1".into(),
                name: "read".into(),
                arguments: "{}".into(),
            }],
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
            error: None,
        });
        let wire = to_wire_messages(&[message]);
        assert!(wire[0]["content"].is_null());
        assert_eq!(wire[0]["tool_calls"][0]["function"]["name"], "read");
    }
}
