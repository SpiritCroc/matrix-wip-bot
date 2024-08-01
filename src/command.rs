use std::{
    self, cmp, str::SplitWhitespace,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use config::Config;
use log::{debug, warn};
use matrix_sdk::{
    Room,
    ruma::events::{
        room::{
            message::{
                OriginalSyncRoomMessageEvent,
                RoomMessageEventContent,
            },
            ImageInfo,
        },
        sticker::StickerEventContent,
    },
};

use crate::users::{is_user_vip, is_user_trusted, is_user_trusted_not_vip};

mod spam;
use spam::{TEXT_SPAM, STICKER_SPAM};

const HELP: &str = "- !help\n\
                    - !ping\n\
                    - !whoami\n\
                    - !sticker [mxc [body]]\n\
                    - !broken-sticker";
const TRUSTED_HELP: &str = "- !spam [count]\n\
                            - !stickerspam [count]\n\
                            - !typing [seconds]";

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
    let content = RoomMessageEventContent::notice_plain(msg);
    if let Err(e) = room.send(content).await {
        warn!("Failed to help in {}: {}", room.room_id(), e);
    }
}

async fn handle_ping(event: OriginalSyncRoomMessageEvent, room: Room) {
    debug!("Got !ping in {} from {}", room.room_id(), event.sender);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let duration = now - u128::from(event.origin_server_ts.0);
    let msg = format!("I'm here (ping took {duration} ms to arrive)");
    let content = RoomMessageEventContent::notice_plain(msg);
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
    debug!("Got typing ({}) in {} from {}, trusted={trusted}", desired_duration.clone().unwrap_or_default(), room.room_id(), event.sender);

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
