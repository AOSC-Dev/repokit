use actix_web::{middleware, post, web, App, HttpRequest, HttpResponse, HttpServer};
use serde::Deserialize;

use teloxide::{prelude::*, types::ParseMode};

#[derive(Clone, Deserialize)]
struct SupportPostPayload {
    id: u64,
    username: String,
    user_id: u64,
    topic_id: u64,
    topic_title: String,
    cooked: String,
    post_number: isize,
}

/// Webhook event payload structure
#[derive(Deserialize)]
pub struct PostPayload {
    post: SupportPostPayload,
}

#[derive(Deserialize, Clone)]
struct DiscourseReviewable {
    id: u64,
    status: i8,
    #[serde(rename = "type")]
    type_: String,
    // user_id: i64,
    created_by_id: i64,
}

#[derive(Deserialize)]
pub struct ReviewPayload {
    reviewable: DiscourseReviewable,
}

#[derive(Clone)]
struct AppContext {
    bot: Bot,
    target: ChatId,
}

/// Flagging handler.
///
/// HTTP status code will be set according to the actions taken by the handler:
/// - `201`: event posted to the Redis event bus.
/// - `204`: event ignored because the reviewable event is not recognized.
#[post("/webhook/flag")]
pub async fn webhook_flag(
    req: HttpRequest,
    payload: web::Json<ReviewPayload>,
    ctx: web::Data<AppContext>,
) -> HttpResponse {
    let event_type = req.headers().get("X-Discourse-Event");
    if let Some(event_type) = event_type {
        if event_type == "reviewable_created" || event_type == "reviewable_transitioned_to" {
            let message = ctx
                .bot
                .send_message(
                    ctx.target,
                    format!(
                        "Flagged: <a href=\"https://bbs.aosc.io/review?sort_order=score\">{}</a>",
                        payload.reviewable.id
                    ),
                )
                .parse_mode(ParseMode::Html)
                .await;
            if let Err(e) = message {
                log::error!("An error occurred while replying to the user: {}", e);
                return HttpResponse::InternalServerError().json(e.to_string());
            }
            return HttpResponse::Created().json("EMIT");
        }
    }
    HttpResponse::NoContent().finish()
}

#[post("/webhook/new-post")]
pub async fn webhook_new_post(
    req: HttpRequest,
    payload: web::Json<PostPayload>,
    ctx: web::Data<AppContext>,
) -> HttpResponse {
    // check if this is the first post
    let is_first_post = if let Some(evt) = req.headers().get("X-Discourse-Event") {
        payload.post.post_number == 1 && evt.as_bytes() == &b"post_created"[..]
    } else {
        false
    };

    if !is_first_post {
        // return early if this is not the first post
        return HttpResponse::NoContent().finish();
    }

    let preview_html = if payload.post.cooked.len() < 2048 {
        ammonia::Builder::new()
            .add_tags(&["img", "figure"])
            .url_relative(ammonia::UrlRelative::RewriteWithBase(
                ammonia::Url::parse("https://bbs.aosc.io").unwrap(),
            ))
            .clean(&payload.post.cooked)
            .to_string()
    } else {
        String::new()
    };

    let message = ctx
        .bot
        .send_message(
            ctx.target,
            format!(
                "<a href=\"https://bbs.aosc.io/t/{}\">{}</a><br/><a href=\"https://bbs.aosc.io/u/{}/summary\">{}</a><br/>{}",
                payload.post.topic_id, &payload.post.topic_title, payload.post.user_id, payload.post.username, &preview_html,
            ),
        )
        .parse_mode(ParseMode::Html)
        .await;
    if let Err(e) = message {
        log::error!("An error occurred while replying to the user: {}", e);
        return HttpResponse::InternalServerError().json(e.to_string());
    }

    HttpResponse::Created().json("EMIT")
}

async fn run() -> std::io::Result<()> {
    let tg_chat_id: i64 = std::env::var("TELEGRAM_CHAT_ID")
        .expect("Please set TELEGRAM_CHAT_ID environment variable!")
        .parse()
        .expect("TELEGRAM_CHAT_ID must be a valid integer!");
    let listen = std::env::var("LISTEN_ADDRESS").expect("LISTEN_ADDRESS not set");

    let bot = Bot::from_env();
    let ctx = AppContext {
        bot,
        target: ChatId(tg_chat_id),
    };

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(web::Data::new(ctx.clone()))
            .service(webhook_flag)
            .service(webhook_new_post)
    })
    .bind(listen)?
    .run()
    .await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    run().await
}
