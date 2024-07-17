use std::{
    self, cmp, str::SplitWhitespace,
    time::{SystemTime, UNIX_EPOCH},
};
use config::Config;
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

pub async fn handle_command(
    cmd: &String,
    args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config,
) {
    match cmd.as_str() {
        "ping" => handle_ping(event, room).await,
        "spam" => handle_spam(args, event, room, config).await,
        "stickerspam" => handle_sticker_spam(args, event, room, config).await,
        "whoami" => handle_whoami(event, room, config).await,
        _ => println!("Ignore unknown command \"{}\" by {} in {}", cmd, event.sender, room.room_id()),
    }
}

async fn handle_ping(event: OriginalSyncRoomMessageEvent, room: Room) {
    println!("Got !ping in {}", room.room_id());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let duration = now - u128::from(event.origin_server_ts.0);
    let msg = format!("I'm here (ping took {duration} ms to arrive)");
    let content = RoomMessageEventContent::text_plain(msg);
    if let Err(e) = room.send(content).await {
        println!("Failed to ping in {}: {}", room.room_id(), e);
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
    println!("Got !spam in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if vip {
        500
    } else if trusted {
        100
    } else {
        let content = RoomMessageEventContent::text_plain("Here be spam");
        if let Err(e) = room.send(content).await {
            println!("Failed to spam in {}: {}", room.room_id(), e);
            return;
        }
        return;
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let custom_count = desired_count.is_ok();
    let count = cmp::min(desired_count.unwrap_or(TEXT_SPAM.len()), max_spam_count);
    for i in 0..count {
        let spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
        let msg = if custom_count { format!("{} - {spam_select}", i+1) } else { spam_select.to_string() };
        let content = RoomMessageEventContent::text_plain(msg);
        if let Err(e) = room.send(content).await {
            println!("Failed to spam in {}: {}", room.room_id(), e);
        }
    }
}

async fn handle_sticker_spam(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    println!("Got !stickerspam in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if vip {
        500
    } else if trusted {
        100
    } else {
        1
    };
    let desired_count = args.next().unwrap_or_default().parse::<usize>();
    let count = cmp::min(desired_count.unwrap_or(STICKER_SPAM.len()), max_spam_count);
    for i in 0..count {
        let spam_select = STICKER_SPAM[i % STICKER_SPAM.len()];
        let text_spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
        let content = StickerEventContent::new(
            text_spam_select.to_string(), // body
            ImageInfo::new(),
            spam_select.into(), // mxc
        );
        if let Err(e) = room.send(content).await {
            println!("Failed to stickerspam in {}: {}", room.room_id(), e);
            return
        }
    }
    let content = RoomMessageEventContent::notice_plain("Done!"); // TODO notice
    if let Err(e) = room.send(content).await {
        println!("Failed to stickerspam in {}: {}", room.room_id(), e);
    }
}

async fn handle_whoami(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: Config
) {
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted(&event.sender, config.clone());
    println!("Got !whoami in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let msg = if vip {
        "You are VIP"
    } else if trusted {
        "You look trustworty"
    } else {
        "You are nobody"
    };
    let content = RoomMessageEventContent::text_plain(msg);
    if let Err(e) = room.send(content).await {
        println!("Failed to spam in {}: {}", room.room_id(), e);
    }
}
