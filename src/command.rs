use std::{
    self, cmp, str::SplitWhitespace,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;
use config::Config;
use log::{trace, debug, warn, error};
use matrix_sdk::{
    Room,
    ruma::assign,
    ruma::events::{
        room::{
            message::{
                OriginalSyncRoomMessageEvent,
                RoomMessageEventContent,
                AddMentions,
                ReplyWithinThread,
                Relation,
                ImageMessageEventContent,
                MessageType,
            },
            ImageInfo,
            MediaSource,
            ThumbnailInfo,
        },
        relation::InReplyTo,
        sticker::StickerEventContent,
    },
};
use rand;

use crate::{
    users::{is_user_vip, is_user_trusted, is_user_trusted_not_vip},
    image_generator,
    WipContext,
    bridge::{BridgeStateContent, BridgeProtocol},
};

mod spam;
use spam::{TEXT_SPAM, STICKER_SPAM};

const FAKE_BRIDGE_KEY: &str = "de.spiritcroc.wipbot";

const HELP: &str = "- `!help` - Print this help\n\
                    - `!ping` - Pong\n\
                    - `!event` - Show event ID of you command message or the message it replies to\n\
                    - `!whoami` - View your permission level\n\
                    - `!sticker [mxc [body]]` - Send a sticker\n\
                    - `!broken-sticker` - Send a sticker with empty url\n\
                    - `!bridge-id [id]` - Set or clear a `m.bridge` state event with a given bridge_id";
const TRUSTED_HELP: &str = "- `!spam [count [delay_seconds]]` - Send lots of text messasges\n\
                            - `!stickerspam [count]` - Send lots of stickers\n\
                            - `!image [width [height [info_width info_height]]]` - Send an image that you have never seen before\n\
                            - `!imagespam [count [width [height]]]` - Like `!image` but more of that\n\
                            - `!thumb [width [height]]` - Like `!image` but with an added thumbnail\n\
                            - `!thread [count]` - Send lots of text messages in a thread\n\
                            - `!reply [count]` - Send lots of text messages as replies\n\
                            - `!typing [seconds]` - Send typing indicator";

pub async fn handle_command(
    cmd: &String,
    args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    context: WipContext,
) {
    match cmd.as_str() {
        "help" => handle_help(event, room, context.config).await,
        "ping" => handle_ping(event, room).await,
        "event" => handle_event_id(event, room).await,
        "id" => handle_event_id(event, room).await,
        "spam" => handle_spam(args, event, room, context.config).await,
        "stickerspam" => handle_sticker_spam(args, event, room, context.config).await,
        "sticker" => handle_sticker(args, event, room).await,
        "image" => handle_image_spam_with_count(1, args, event, room, context, false).await,
        "thumb" => handle_image_spam_with_count(1, args, event, room, context, true).await,
        "thumbnail" => handle_image_spam_with_count(1, args, event, room, context, true).await,
        "imagespam" => handle_image_spam(args, event, room, context).await,
        "thread" => handle_thread_spam(args, event, room, context.config).await,
        "reply" => handle_reply_spam(args, event, room, context).await,
        "replies" => handle_reply_spam(args, event, room, context).await,
        "typing" => handle_typing(args, event, room, context.config).await,
        "broken-sticker" => handle_sticker_broken(event, room).await,
        "whoami" => handle_whoami(event, room, context.config).await,
        "bridge-id" => handle_bride_id(args, event, room).await,
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

async fn handle_event_id(event: OriginalSyncRoomMessageEvent, room: Room) {
    debug!("Got !event in {} from {}", room.room_id(), event.sender);
    let event_id = if let Some(Relation::Reply { in_reply_to }) = event.content.relates_to {
        in_reply_to.event_id
    } else {
        event.event_id
    };
    let msg_html = format!("<pre><code>{}</code></pre>", event_id);
    let content = assign!(RoomMessageEventContent::notice_html(event_id.clone(), msg_html), {
        relates_to: Some(Relation::Reply { in_reply_to: InReplyTo::new(event_id) }),
    });
    if let Err(e) = room.send(content).await {
        warn!("Failed to send event ID in {}: {}", room.room_id(), e);
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
    let max_spam_count = if room.is_public().unwrap_or(true) {
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
    let desired_count = desired_count.unwrap_or(TEXT_SPAM.len());
    let mut count = cmp::min(desired_count, max_spam_count);
    let mut effective_delay: u64 = 0;
    let spam_delay = args.next().unwrap_or_default().parse::<u64>().map(|d| {
        let max_delay = config.get::<u64>("bot.delay_spam.limit").unwrap_or(20);
        effective_delay = cmp::max(cmp::min(d, max_delay), 1);
        let max_count_by_delay = max_delay / effective_delay;
        count = cmp::min(count, max_count_by_delay.try_into().unwrap_or(usize::MAX));
        Duration::from_secs(effective_delay)
    });
    // Tell the user when limitting or delaying
    if count < desired_count || effective_delay > 0 {
        let room_clone = room.clone();
        //tokio::spawn(async move {
            let delay_note = if count == desired_count {
                format!("I will spam {count} messages delayed by {effective_delay}s")
            } else if effective_delay > 0 {
                format!("Limit notice: I will spam {count} messages delayed by {effective_delay}s")
            } else {
                format!("Limit notice: I will spam {count} messages")
            };
            let content = RoomMessageEventContent::notice_plain(delay_note);
            if let Err(e) = room_clone.send(content).await {
                warn!("Failed to send spam delay note in {}: {}", room_clone.room_id(), e);
            }
        //});
    }
    tokio::spawn(async move {
        for i in 0..count {
            if let Ok(sleep_duration) = spam_delay {
                sleep(sleep_duration).await;
            };
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
    let max_spam_count = if room.is_public().unwrap_or(true) {
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
    context: WipContext,
) {
    let config = context.config;
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !reply in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public().unwrap_or(true) {
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
    let mut reply_to = event.event_id;
    tokio::spawn(async move {
        for i in 0..count {
            let spam_select = TEXT_SPAM[i % TEXT_SPAM.len()];
            let msg = if custom_count { format!("{} - {spam_select}", i+1) } else { spam_select.to_string() };
            let content = assign!(RoomMessageEventContent::new(MessageType::text_plain(msg)), {
                relates_to: Some(Relation::Reply { in_reply_to: InReplyTo::new(reply_to) }),
            });
            match room.send(content.clone()).await {
                Err(e) => {
                    warn!("Failed to thread-spam in {}: {}", room.room_id(), e);
                    return;
                }
                Ok(r) => {
                    reply_to = r.event_id;
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
    let max_spam_count = if room.is_public().unwrap_or(true) {
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
    context: WipContext,
) {
    let desired_count = args.next().unwrap_or_default().parse::<usize>().unwrap_or(3);
    handle_image_spam_with_count(desired_count, args, event, room, context, false).await;
}

async fn handle_image_spam_with_count(
    desired_count: usize,
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    context: WipContext,
    with_thumbnail: bool,
) {
    let config = context.config;
    let vip = is_user_vip(&event.sender, config.clone());
    let trusted = is_user_trusted_not_vip(&event.sender, config.clone());
    debug!("Got !image {desired_count} in {} from {}, vip={vip}, trusted={trusted}", room.room_id(), event.sender);
    let max_spam_count = if room.is_public().unwrap_or(true) {
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
    let claimed_width = args.next().unwrap_or_default().parse::<usize>().unwrap_or(width);
    let claimed_height = args.next().unwrap_or_default().parse::<usize>().unwrap_or(height);
    let text_override = args.next().map(|t| t.to_string());
    let font_size = (if count == 1 { 42.0 } else { 64.0 }) * ((width as f64)/150.0);

    let media_client = context.media_client.unwrap_or_else(|| room.client());

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

            let (thumbnail_info, thumbnail_uri) = if with_thumbnail {
                let thumbnail_text = format!("t.{}", text.unwrap_or_default());
                let thumb_width = width/2;
                let thumb_height = height/2;
                let thumb_font_size = font_size/2.0;
                match image_generator::create_text_image(
                    Some(&thumbnail_text),
                    &background_color,
                    "#ffffff",
                    thumb_width,
                    thumb_height,
                    thumb_font_size
                ).await {
                    Err(e) => {
                        error!("Failed to generate thumbnail image: {}", e);
                        (None, None)
                    }
                    Ok(thumb_image) => {
                        let thumbnail_info = assign!(ThumbnailInfo::new(), {
                            width: thumb_width.try_into().ok(),
                            height: thumb_height.try_into().ok(),
                            size: thumb_image.len().try_into().ok(),
                            mimetype: Some(mime::IMAGE_PNG.to_string()),
                        });

                        match media_client.media().upload(
                            &mime::IMAGE_PNG,
                            thumb_image,
                            None,
                        ).await {
                            Ok(u) => {
                                (
                                    Some(Box::new(ThumbnailInfo::from(thumbnail_info))),
                                    Some(u.content_uri)
                                )
                            },
                            Err(e) => {
                                error!("Failed to upload thumbnail: {}", e);
                                (None, None)
                            }
                        }
                    }
                }
            } else {
                (None, None)
            };

            let thumbnail_source = thumbnail_uri.clone().map(MediaSource::Plain);

            let image_info = assign!(ImageInfo::new(), {
                width: claimed_width.try_into().ok(),
                height: claimed_height.try_into().ok(),
                size: image_size.try_into().ok(),
                // TODO: random blurhash? (random is better for testing than a solid color like the
                //  generated images) - https://github.com/woltapp/blurhash/blob/master/Algorithm.md
                blurhash: Some("LEDuYo=b9]tP02xt}?jGEj9]4;of".to_string()),
                mimetype: Some(mime::IMAGE_PNG.essence_str().to_string()),
                thumbnail_info: thumbnail_info,
                thumbnail_source: thumbnail_source,
            });

            let image_upload = match media_client.media().upload(
                &mime::IMAGE_PNG,
                image,
                None,
            ).await {
                Ok(u) => u,
                Err(e) => {
                    error!("Failed to upload image: {}", e);
                    return
                }
            };

            let image_content = ImageMessageEventContent::plain(
                format!("{i}.png"),
                image_upload.content_uri.clone(),
            ).info(Some(Box::new(image_info)));

            let message = RoomMessageEventContent::new(
                MessageType::Image(image_content)
            );

            if let Err(e) = room.send(message).await {
                warn!("Failed to send image in {}: {}", room.room_id(), e);
                return;
            }

            trace!("Successfully sent image with size {image_size}, mxc {} and thumbnail {:?}", image_upload.content_uri, thumbnail_uri);
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

async fn handle_bride_id(
    mut args: SplitWhitespace<'_>,
    event: OriginalSyncRoomMessageEvent,
    room: Room,
) {
    let bridge_id = args.next();
    debug!("Got !bride_id ({}) in {} from {}", bridge_id.clone().unwrap_or_default(), room.room_id(), event.sender);
    let content = BridgeStateContent {
        bridgebot: Some(room.own_user_id().into()),
        creator: Some(room.own_user_id().into()),
        protocol: bridge_id.map(|id|
            BridgeProtocol {
                id: id.to_string(),
                displayname: id.to_string(),
            }
        ),
    };
    tokio::spawn(async move {
        if let Err(e) = room.send_state_event_for_key(FAKE_BRIDGE_KEY, content).await {
            warn!("Failed to send bridge_id in {}: {}", room.room_id(), e);
            let content = RoomMessageEventContent::text_plain("Failed to set bridge-id");
            if let Err(e) = room.send(content).await {
                warn!("Failed to send bridge_id error message in {}: {}", room.room_id(), e);
            }
        } else {
            let content = RoomMessageEventContent::notice_markdown("Bridge content updated");
            if let Err(e) = room.send(content).await {
                warn!("Failed to send bridge_id success message in {}: {}", room.room_id(), e);
            }
        }
    });
}
