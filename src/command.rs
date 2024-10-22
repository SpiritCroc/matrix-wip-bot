use std::{
    self, cmp, str::SplitWhitespace,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use config::Config;
use log::{trace, debug, warn, error};
use matrix_sdk::{
    Room,
    ruma::events::{
        room::{
            message::{
                OriginalSyncRoomMessageEvent,
                RoomMessageEventContent,
                AddMentions,
                ReplyWithinThread,
                ForwardThread,
            },
            ImageInfo,
        },
        sticker::StickerEventContent,
    },
    attachment::{
        AttachmentConfig,
        AttachmentInfo,
        BaseImageInfo,
    },
};
use rand;

use crate::{
    users::{is_user_vip, is_user_trusted, is_user_trusted_not_vip},
    image_generator,
};

mod spam;
use spam::{TEXT_SPAM, STICKER_SPAM};

const HELP: &str = "- `!help` - Print this help\n\
                    - `!ping` - Pong\n\
                    - `!whoami` - View your permission level\n\
                    - `!sticker [mxc [body]]` - Send a sticker\n\
                    - `!broken-sticker` - Send a sticker with empty url";
const TRUSTED_HELP: &str = "- `!spam [count]` - Send lots of text messasges\n\
                            - `!stickerspam [count]` - Send lots of stickers\n\
                            - `!image [width [height]]` - Send an image that you have never seen before\n\
                            - `!imagespam [count [width [height]]]` - Like `!image` but more of that\n\
                            - `!thread [count]` - Send lots of text messages in a thread\n\
                            - `!reply [count]` - Send lots of text messages as replies\n\
                            - `!typing [seconds]` - Send typing indicator";

pub async fn handle_command(
    cmd: &String,
    args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config,
) {
    match cmd.as_str() {
        "help" => handle_help(event, room, config).await,
        "ping" => handle_ping(event, room).await,
        "spam" => handle_spam(args, event, room, config).await,
        "stickerspam" => handle_sticker_spam(args, event, room, config).await,
        "sticker" => handle_sticker(args, event, room).await,
        "image" => handle_image_spam_with_count(1, args, event, room, config).await,
        "imagespam" => handle_image_spam(args, event, room, config).await,
        "thread" => handle_thread_spam(args, event, room, config).await,
        "reply" => handle_reply_spam(args, event, room, config).await,
        "replies" => handle_reply_spam(args, event, room, config).await,
        "typing" => handle_typing(args, event, room, config).await,
        "broken-sticker" => handle_sticker_broken(event, room).await,
        "whoami" => handle_whoami(event, room, config).await,
        _ => debug!("Ignore unknown command \"{}\" by {} in {}", cmd, event.sender, room.room_id()),
    }
}

async fn handle_help(event: OriginalSyncRoomMessageEvent, room: Room, config: Config) {
    let trusted = is_user_trusted(&event.sender, config.clone());
    debug!("Got !help in {} from {}, trusted={trusted}", room.room_id(), event.sender);
    let msg = if trusted {
        format!("{}\n{}", HELP, TRUSTED_HELP)
    } else {
        HELP.to_string()
    };
    let content = RoomMessageEventContent::notice_markdown(msg);
    if let Err(e) = room.send(content).await {
        warn!("Failed to help in {}: {}", room.room_id(), e);
    }
}

async fn handle_ping(event: OriginalSyncRoomMessageEvent, room: Room) {
    debug!("Got !ping in {} from {}", room.room_id(), event.sender);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let duration = now - u128::from(event.origin_server_ts.0);
    let msg_plain = format!("I'm here (ping took {duration} ms to arrive)");
    let msg_html = format!(
        "<a href='https://matrix.to/#/{}'>{}</a>: Pong! (<a href='https://matrix.to/#/{}/{}'>ping</a> took {duration} ms to arrive)",
        event.sender,
        event.sender,
        room.room_id(),
        event.event_id,
    );
    let content = RoomMessageEventContent::notice_html(msg_plain, msg_html);
    if let Err(e) = room.send(content).await {
        warn!("Failed to ping in {}: {}", room.room_id(), e);
    }
}

async fn handle_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !spam in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public() {
        // No spam in public rooms please...
        // But showing a single spam sticker wouldn't hurt?
        handle_sticker_spam(args, event, room, config).await;
        return;
    } else if vip {
        config.get::<usize>("bot.text_spam.vip_limit").unwrap_or(500)
    } else if trusted {
        config.get::<usize>("bot.text_spam.trusted_limit").unwrap_or(100)
    } else {
        let content = RoomMessageEventContent::text_plain("Here be spam");
        if let Err(e) = room.send(content).await {
            warn!("Failed to spam in {}: {}", room.room_id(), e);
            return;
        }
        return;
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let custom_count = desired_count.is_ok();
    let count = cmp::min(desired_count.unwrap_or(TEXT_SPAM.len()), max_spam_count);
    tokio::spawn(async move {
        for i in 0..count {
            let spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
            let msg = if custom_count { format!("{} - {spam_select}", i+1) } else { spam_select.to_string() };
            let content = RoomMessageEventContent::text_plain(msg);
            if let Err(e) = room.send(content).await {
                warn!("Failed to spam in {}: {}", room.room_id(), e);
                return;
            }
        }
    });
}

async fn handle_thread_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !thread in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public() {
        // TODO single message fallback
        return;
    } else if vip {
        config.get::<usize>("bot.text_spam.vip_limit").unwrap_or(500)
    } else if trusted {
        config.get::<usize>("bot.text_spam.trusted_limit").unwrap_or(100)
    } else {
        // TODO single message fallback
        return;
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let custom_count = desired_count.is_ok();
    let count = cmp::min(desired_count.unwrap_or(TEXT_SPAM.len()), max_spam_count);
    let full_orig_event = event.into_full_event(room.room_id().to_owned());
    tokio::spawn(async move {
        for i in 0..count {
            let spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
            let msg = if custom_count { format!("{} - {spam_select}", i+1) } else { spam_select.to_string() };
            let content = RoomMessageEventContent::text_plain(msg).make_for_thread(
                &full_orig_event,
                ReplyWithinThread::No,
                AddMentions::No,
            );
            if let Err(e) = room.send(content).await {
                warn!("Failed to thread-spam in {}: {}", room.room_id(), e);
                return;
            }
        }
    });
}

async fn handle_reply_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !reply in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public() {
        // TODO single reply fallback
        return;
    } else if vip {
        config.get::<usize>("bot.text_spam.vip_limit").unwrap_or(500)
    } else if trusted {
        config.get::<usize>("bot.text_spam.trusted_limit").unwrap_or(100)
    } else {
        // TODO single reply fallback
        return;
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let custom_count = desired_count.is_ok();
    let count = cmp::min(desired_count.unwrap_or(1), max_spam_count);
    let reply_to = event.into_full_event(room.room_id().to_owned());
    tokio::spawn(async move {
        for i in 0..count {
            let spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
            let msg = if custom_count { format!("{} - {spam_select}", i+1) } else { spam_select.to_string() };
            let content = RoomMessageEventContent::text_plain(msg).make_reply_to(
                &reply_to,
                ForwardThread::No,
                AddMentions::No,
            );
            match room.send(content).await {
                Err(e) => {
                    warn!("Failed to thread-spam in {}: {}", room.room_id(), e);
                    return;
                }
                Ok(_r) => {
                    // TODO reply_to = ...;
                }
            }
        }
    });
}

async fn handle_sticker_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !stickerspam in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public() {
        1
    } else if vip {
        config.get::<usize>("bot.sticker_spam.vip_limit").unwrap_or(500)
    } else if trusted {
        config.get::<usize>("bot.sticker_spam.trusted_limit").unwrap_or(100)
    } else {
        1
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let count = cmp::min(desired_count.unwrap_or(STICKER_SPAM.len()), max_spam_count);
    tokio::spawn(async move {
        for i in 0..count {
            let spam_select = STICKER_SPAM[i % STICKER_SPAM.len()];
            let text_spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
            let content = StickerEventContent::new(
                text_spam_select.to_string(), // body
                ImageInfo::new(),
                spam_select.into(), // mxc
            );
            if let Err(e) = room.send(content).await {
                warn!("Failed to stickerspam in {}: {}", room.room_id(), e);
                return
            }
        }
        if count > STICKER_SPAM.len() {
            let content = RoomMessageEventContent::notice_plain("Done!");
            if let Err(e) = room.send(content).await {
                warn!("Failed to stickerspam in {}: {}", room.room_id(), e);
            }
        }
    });
}

async fn handle_sticker(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
) {
    debug!("Got !sticker in {} from {}", room.room_id(), event.sender);
    let mxc = args.next().unwrap_or("mxc://spiritcroc.de/mkJFKqrNzBGBcILPTIPlTPOV");
    let body = args.next().unwrap_or("Sticker");
    let content = StickerEventContent::new(
        body.to_string(),
        ImageInfo::new(),
        mxc.into(),
    );
    if let Err(e) = room.send(content).await {
        warn!("Failed to stickerspam in {}: {}", room.room_id(), e);
        return
    }
}

async fn handle_sticker_broken(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
) {
    debug!("Got !broken-sticker in {} from {}", room.room_id(), event.sender);
    let content = StickerEventContent::new(
        "Broken sticker".to_string(),
        ImageInfo::new(),
        "".into(), // mxc
    );
    if let Err(e) = room.send(content).await {
        warn!("Failed to stickerspam in {}: {}", room.room_id(), e);
        return
    }
}

async fn handle_image_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let desired_count = args.next().unwrap_or_default().parse::<usize>().unwrap_or(3);
    handle_image_spam_with_count(desired_count, args, event, room, config).await;
}

async fn handle_image_spam_with_count(
    desired_count: usize,
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !image in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public() {
        1
    } else if vip {
        config.get::<usize>("bot.image_spam.vip_limit").unwrap_or(50)
    } else if trusted {
        config.get::<usize>("bot.image_spam.trusted_limit").unwrap_or(00)
    } else {
        1
    };
    let max_size = config.get::<usize>("bot.image_spam.max_size").unwrap_or(500);
    let count = cmp::min(desired_count, max_spam_count);
    let width = cmp::min(args.next().unwrap_or_default().parse::<usize>().unwrap_or(150), max_size);
    let height = cmp::min(args.next().unwrap_or_default().parse::<usize>().unwrap_or(width), max_size);
    let text_override = args.next().map(|t| t.to_string());
    let font_size = if count == 1 { 42.0 } else { 64.0 };
    tokio::spawn(async move {
        for i in 1..=count {
            let text = if count == 1 { text_override.clone() } else { Some(i.to_string()) };
            let background_color: u32 = rand::random();
            let background_color = format!("#{:06x}", background_color % 0xffffff);
            let image = image_generator::create_text_image(
                text.as_ref(),
                &background_color,
                "#ffffff",
                width,
                height,
                font_size
            ).await;
            let image = match image {
                Ok(i) => i,
                Err(e) => {
                    error!("Failed to generate image: {}", e);
                    return;
                }
            };
            let image_size = image.len();
            let attachment_info = AttachmentInfo::Image(
                BaseImageInfo {
                    width: width.try_into().ok(),
                    height: height.try_into().ok(),
                    size: image_size.try_into().ok(),
                    blurhash: None
                }
            );
            let attachment_config = AttachmentConfig::new()
                .info(attachment_info);
            if let Err(e) = room.send_attachment(
                &format!("{i}.png"),
                &mime::IMAGE_PNG,
                image,
                attachment_config
            ).await {
                warn!("Failed to imagespam in {}: {}", room.room_id(), e);
                return
            }
            trace!("Successfully sent image with size {image_size}");
        }
    });
}

async fn handle_whoami(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted(&event.sender, config.clone());
    debug!("Got !whoami in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let msg = if vip {
        "You are VIP"
    } else if trusted {
        "You look trustworty"
    } else {
        "You are nobody"
    };
    let content = RoomMessageEventContent::notice_plain(msg);
    if let Err(e) = room.send(content).await {
        warn!("Failed to whoami in {}: {}", room.room_id(), e);
    }
}

async fn handle_typing(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config,
) {
    let trusted = is_user_trusted(&event.sender, config.clone());
    if !trusted {
        return;
    }

    let desired_duration = args.next().unwrap_or_default().parse::<u64>();
    debug!("Got !typing ({}) in {} from {}, trusted={trusted}", desired_duration.clone().unwrap_or_default(), room.room_id(), event.sender);

    let max_duration = config.get::<u64>("bot.typing.max_duration").unwrap_or(20);
    let duration = cmp::min(desired_duration.unwrap_or(5), max_duration);

    // Need to refresh the typing every once in a while:
    // https://spec.matrix.org/v1.11/client-server-api/#put_matrixclientv3roomsroomidtypinguserid
    tokio::spawn(async move {
        let typing_period = 5;
        let mut remaining = duration;
        loop {
            if let Err(e) = room.typing_notice(true).await {
                warn!("Failed to start typing in {}: {}", room.room_id(), e);
                return;
            }
            if remaining <= 0 {
                break;
            }
            if remaining > typing_period {
                sleep(Duration::from_secs(typing_period)).await;
                remaining -= typing_period;
            } else {
                sleep(Duration::from_secs(remaining)).await;
                break;
            }
        }
        if let Err(e) = room.typing_notice(false).await {
            warn!("Failed to stop typing in {}: {}", room.room_id(), e);
            return;
        }
        let msg = format!("I was just typing for {duration} seconds!");
        let content = RoomMessageEventContent::notice_plain(msg);
        if let Err(e) = room.send(content).await {
            warn!("Failed to finalize typing in {}: {}", room.room_id(), e);
        } else {
            debug!("Finished typing after {duration} in {}", room.room_id());
        }
    });
}
