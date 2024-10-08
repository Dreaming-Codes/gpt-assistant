use async_openai::config::OpenAIConfig;
use async_openai::types::*;
use async_openai::Client;
use async_stream::stream;
use iced::futures::Stream;
use iced::theme::Palette;
use iced::widget::{container, horizontal_space, Space, Text};
use iced::{window, Color, Element, Point, Subscription, Task, Theme};
use image::{ImageFormat, RgbaImage};
use rdev::{listen, EventType, Key};
use std::io::Cursor;
use std::thread;
use async_openai::error::OpenAIError;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD};
use thiserror::Error;
use tokio::sync::mpsc;
use xcap::{Monitor, XCapError};

// Predefined Colors
const IDLE_COLOR: Color = Color::from_rgb(0.996, 0.871, 0.545);
const LOADING_COLOR: Color = Color::from_rgb(0.0, 0.5, 0.0);
const ERROR_COLOR: Color = Color::from_rgb(0.8, 0.0, 0.0);

pub fn main() -> iced::Result {
    iced::application("overlay", Assistant::update, Assistant::view).theme(|_| Theme::custom(
        "main".to_string(),
        Palette {
            background: Color::TRANSPARENT,
            ..Theme::default().palette()
        },
    )).antialiasing(true).window(window::Settings {
        transparent: true,
        resizable: false,
        decorations: false,
        level: window::Level::AlwaysOnTop,
        position: window::Position::Specific(Point::new(0f32, 0f32)),
        ..Default::default()
    }).subscription(Assistant::keyboard_subscription).run_with(Assistant::new)
}

#[derive(Debug, Clone, Copy)]
enum State {
    Idle,
    Error,
    Loading,
}

struct Assistant {
    visible: bool,
    current_text: Option<String>,
    state: State,
}

impl Default for Assistant {
    fn default() -> Self {
        Self {
            visible: true,
            current_text: None,
            state: State::Idle,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ShowText(Option<String>),
    ToggleVisibility,
    SetState(State),
}

impl Assistant {
    fn new() -> (Self, Task<Message>) {
        (
            Self::default(),
            window::get_latest().and_then(|window| {
                Task::batch([
                    window::change_mode(window, window::Mode::Fullscreen),
                    window::enable_mouse_passthrough(window),
                ])
            }),
        )
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::ShowText(text) => self.current_text = text,
            Message::SetState(state) => self.state = state,
            Message::ToggleVisibility => self.visible = !self.visible,
        }
    }

    fn keyboard_subscription(&self) -> Subscription<Message> {
        Subscription::run(listen_keyboard)
    }

    fn view(&self) -> Element<Message> {
        if self.visible {
            if let Some(text) = &self.current_text {
                container(Text::new(text).size(20).color(Color::BLACK)).into()
            } else {
                container(Space::new(25, 25)).style(|t: &Theme| container::Style {
                    background: Some(match self.state {
                        State::Idle => IDLE_COLOR,
                        State::Loading => LOADING_COLOR,
                        State::Error => ERROR_COLOR,
                    }.into()),
                    ..container::transparent(t)
                }).into()
            }
        } else {
            horizontal_space().into()
        }
    }
}

fn image_to_base64(img: &RgbaImage) -> String {
    let mut image_data: Vec<u8> = Vec::new();
    img.write_to(&mut Cursor::new(&mut image_data), ImageFormat::Png).unwrap();
    format!("data:image/png;base64,{}", STANDARD.encode(image_data))
}

fn listen_keyboard() -> impl Stream<Item = Message> {
    let (event_schan, mut event_rchan) = mpsc::unbounded_channel();
    let (msg_schan, mut msg_rchan) = mpsc::unbounded_channel();

    let _listener = thread::spawn(move || {
        listen(move |event| {
            event_schan.send(event).unwrap_or_else(|e| println!("Could not send event {:?}", e));
        }).expect("Could not listen");
    });

    let mut ctrl_pressed = false;
    let client = Client::new();

    stream! {
        loop {
            tokio::select! {
                Some(event) = event_rchan.recv() => {
                    match event.event_type {
                        EventType::KeyPress(key) => match key {
                            Key::ControlLeft => ctrl_pressed = true,
                            Key::ControlRight => yield Message::ShowText(None),
                            Key::KeyO | Key::KeyI if ctrl_pressed => {
                                yield Message::SetState(State::Loading);
                                yield Message::ShowText(None);

                                let client = client.clone();
                                let msg_schan = msg_schan.clone();

                                tokio::spawn(async move {
                                    let result: Result<String, AppError> = async {
                                        let base64 = tokio::task::spawn_blocking(|| {
                                            let monitors = Monitor::all()?;
                                            let monitor = monitors.first().ok_or_else(|| AppError::NoMonitors)?;
                                            let image = monitor.capture_image()?;
                                            Ok::<_, AppError>(image_to_base64(&image))
                                        })
                                        .await??;

                                        let answer = if key == Key::KeyO {
                                            let quiz_text = extract_text_from_image(&client, base64).await?;
                                            get_exact_answer(&client, quiz_text).await?
                                        } else {
                                            direct_answer_from_image(&client, base64).await?
                                        };

                                        Ok(answer)
                                    }.await;

                                    match result {
                                        Ok(answer) => {
                                            msg_schan.send(Message::SetState(State::Idle)).unwrap();
                                            msg_schan.send(Message::ShowText(Some(answer))).unwrap();
                                        },
                                        Err(e) => {
                                            eprint!("Error getting answer: {:?}", e);
                                            msg_schan.send(Message::SetState(State::Error)).unwrap();
                                        }
                                    }
                                });
                            },
                            Key::Alt | Key::AltGr => yield Message::ToggleVisibility,
                            _ => {}
                        },
                        EventType::KeyRelease(Key::ControlLeft | Key::ControlRight) => ctrl_pressed = false,
                        _ => {}
                    }
                },
                Some(msg) = msg_rchan.recv() => {
                    yield msg;
                }
            }
        }
    }
}

async fn direct_answer_from_image(client: &Client<OpenAIConfig>, base64: String) -> Result<String, AppError> {
    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-4o")
        .messages(vec![
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::Text(
                            ChatCompletionRequestMessageContentPartTextArgs::default()
                                .text("Answer to the test in the image attached, be concise and to the point")
                                .build()?
                        ),
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImageArgs::default()
                                .image_url(base64)
                                .build()?
                        )
                    ]))
                    .build()?
            )
        ])
        .build()?;

    let response = client.chat().create(request).await?;

    Ok(response.choices.into_iter().map(|choice| choice.message.content.unwrap()).collect::<Vec<String>>().join(""))
}

async fn extract_text_from_image(client: &Client<OpenAIConfig>, base64: String) -> Result<String, AppError> {
    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-4o")
        .messages(vec![
            ChatCompletionRequestMessage::System(
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("Your task is to extract text from the quiz on screen. If there's an image, you should explain the content of it for someone to be able to answer the question without having to look at the image. If there are multiple choices, transcribe those too. Ignore other things on screen.")
                    .build()?
            ),
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(ChatCompletionRequestUserMessageContent::Array(vec![
                        ChatCompletionRequestUserMessageContentPart::ImageUrl(
                            ChatCompletionRequestMessageContentPartImageArgs::default()
                                .image_url(base64)
                                .build()?
                        )
                    ]))
                    .build()?
            )
        ])
        .build()?;

    let response = client.chat().create(request).await?;

    Ok(response.choices.into_iter().map(|choice| choice.message.content.unwrap()).collect::<Vec<String>>().join(""))
}

async fn get_exact_answer(client: &Client<OpenAIConfig>, text: String) -> Result<String, AppError> {
    let request = CreateChatCompletionRequestArgs::default()
        .model("o1-preview")
        .messages(vec![
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content("Your task is to provide only the exact answer without any explanations.")
                    .build()?
            ),
            ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessageArgs::default()
                    .content(text)
                    .build()?
            ),
        ])
        .build()?;

    let response = client.chat().create(request).await?;

    Ok(response.choices.into_iter().map(|choice| choice.message.content.unwrap()).collect::<Vec<String>>().join(""))
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("OpenAI API Error: {0}")]
    ApiError(#[from] OpenAIError),

    #[error("Task Error: {0}")]
    TaskError(#[from] tokio::task::JoinError),

    #[error("No monitors found")]
    NoMonitors,

    #[error(transparent)]
    ScreenshotError(#[from] XCapError),
}
