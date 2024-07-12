use std::str::SplitWhitespace;
use matrix_sdk::{
    Room,
    ruma::events::room::message::{
        RoomMessageEventContent, OriginalSyncRoomMessageEvent,
    },
};

pub async fn handle_command(cmd: &String, args: SplitWhitespace<'_>, event: OriginalSyncRoomMessageEvent, room: Room) {
    match cmd.as_str() {
        "ping" => handle_ping(room).await,
        "spam" => handle_spam(args, room).await,
        _ => println!("Ignore unknown command \"{}\" by {} in {}", cmd, event.sender, room.room_id()),
    }
}

async fn handle_ping(room: Room) {
    println!("Got !ping in {}", room.room_id());
    let content = RoomMessageEventContent::text_plain("I'm here");
    if let Err(e) = room.send(content).await {
        println!("Failed to ping in {}: {}", room.room_id(), e);
    }
}

async fn handle_spam(args: SplitWhitespace<'_>, room: Room) {
    println!("Got !spam in {}", room.room_id());
    let content = RoomMessageEventContent::text_plain("Here be spam");
    if let Err(e) = room.send(content).await {
        println!("Failed to spam in {}: {}", room.room_id(), e);
    }
}
