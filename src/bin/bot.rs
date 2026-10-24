//! A player with no window: joins a game hosted with `fps_prototype --host`, and follows the host's player
//! about, going round walls and through doors, to show that the networking works. It says what it is doing
//! once a second.
//!
//!     cargo run --bin bot                           join the game on this machine
//!     cargo run --bin bot -- 192.168.1.20           join the game on another (port 7777)
//!     cargo run --bin bot -- 192.168.1.20:9000 --name Rex
//!
//! It shoots the zombies it can see, and the zombies go after it as they do you; if it is brought down it lies
//! there for a while and then gets up again. While it runs, type `stay`, `follow`, `hold fire`, `fire at will`
//! or `quit` and press Enter.

use std::io::BufRead;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fps_prototype::bot::Bot;
use fps_prototype::collision::Colliders;
use fps_prototype::map::TerrainMap;
use fps_prototype::net::DEFAULT_PORT;
use fps_prototype::net_host::with_port;
use fps_prototype::params::GenParams;
use fps_prototype::world::headless_colliders;

fn main() {
    let mut address = format!("127.0.0.1:{DEFAULT_PORT}");
    let mut name = "Botty".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => name = args.next().unwrap_or(name),
            "--help" | "-h" => {
                println!("bot [address[:port]] [--name NAME]    join a game hosted with `fps_prototype --host` and follow its player");
                println!("While it runs, type stay, follow, hold fire, fire at will or quit and press Enter.");
                return;
            }
            other => address = with_port(other),
        }
    }
    println!("Joining {address} as {name}...");
    let mut bot = match Bot::join(address.as_str(), &name, Duration::from_secs(10)) {
        Ok(bot) => bot,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!("Let in as player {}. Making the map the game is on (seed {}); this takes several seconds...", bot.id, bot.seed);
    // The world is made on another thread while this one keeps the host hearing from it.
    let seed = bot.seed;
    let making = std::thread::spawn(move || {
        let params = GenParams::default();
        let map = TerrainMap::generate(seed, &params);
        let colliders = headless_colliders(&map, &params);
        (map, colliders)
    });
    while !making.is_finished() {
        bot.hold(0.05);
        std::thread::sleep(Duration::from_millis(50));
    }
    let (map, colliders): (TerrainMap, Colliders) = making.join().expect("the world could be made");
    bot.arrive(&map, &colliders);
    println!("Ready: {} solid things in the world. Type stay, follow, hold fire, fire at will or quit.", colliders.len());

    let (typed, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if typed.send(line.trim().to_lowercase()).is_err() {
                break;
            }
        }
    });

    let mut last = Instant::now();
    let mut next_report = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.25);
        last = now;
        while let Ok(command) = commands.try_recv() {
            match command.as_str() {
                "quit" | "q" | "exit" => {
                    bot.leave();
                    println!("Left the game.");
                    return;
                }
                "stay" => {
                    bot.stay = true;
                    println!("Staying here.");
                }
                "follow" | "come" => {
                    bot.stay = false;
                    println!("Following you.");
                }
                "hold fire" | "hold" | "cease fire" => {
                    bot.shoot = false;
                    println!("Holding fire.");
                }
                "fire at will" | "fire" => {
                    bot.shoot = true;
                    println!("Firing at will.");
                }
                "" => {}
                other => println!("I know stay, follow, hold fire, fire at will and quit, not {other:?}."),
            }
        }
        bot.tick(dt, &map, &colliders);
        if now >= next_report {
            next_report = now + Duration::from_secs(1);
            println!("{}", bot.report());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
