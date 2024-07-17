#[cfg(not(test))]
mod config;

#[cfg(test)]
mod config {
    pub const EMAIL: &str = "my-email@example.org";
    pub const CLIENT_ID: &str = "some_client_id";
    pub const CLIENT_SECRET: &str = "client_secret";
}

mod tokens;

mod meetings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut only_link = false;
    let mut debug = false;
    let mut json = false;
    let mut machine_full = false;
    let mut additional_links = false;
    let mut all_meets = false;

    std::env::args().skip(1).for_each(|opt| match opt.as_str() {
        "-m" => only_link = true,
        "-d" => debug = true,
        "-j" => json = true,
        "-mf" => machine_full = true,
        "-al" => additional_links = true,
        "-a" => all_meets = true,
        _ => (),
    });

    if json {
        match meetings::json().await {
            Ok(json) => {
                println!("{}", json);
                std::process::exit(0);
            }
            Err(err) => {
                println!("Error: {}", err);
                std::process::exit(1);
            }
        };
    }

    if machine_full {
        let tokens = tokens::Tokens::load();

        if let Ok(tokens) = tokens.and_then(|t| t.refresh()) {
            let result = meetings::retrieve_with_tokens(false, tokens)
                .await?
                .map(|m| serde_json::to_string(&m).unwrap())
                .unwrap_or_else(String::new);

            println!("{result}");
            std::process::exit(0);
        }

        eprintln!("Error: Could not refresh tokens");
        std::process::exit(1);
    }

    if additional_links {
        let tokens = tokens::Tokens::load();

        if let Ok(tokens) = tokens.and_then(|t| t.refresh()) {
            let result = meetings::retrieve_with_tokens(false, tokens)
                .await?
                .map(|m| m.get_other_links().join(" "))
                .unwrap_or_else(String::new);

            println!("{result}");
            std::process::exit(0);
        }

        eprintln!("Error: Could not refresh tokens");
        std::process::exit(1);
    }

    if all_meets {
        for meet in meetings::retrieve_all().await? {
            println!("{}\n", meet);
        }
        std::process::exit(0);
    }

    let meeting = meetings::retrieve(debug).await?;

    if only_link {
        meeting.and_then(|m| m.get_link()).map(|link| {
            println!("{}", link);
            std::process::exit(0);
        });
        std::process::exit(1);
    } else {
        match meeting {
            None => println!("Non ci sono appuntamenti"),
            Some(meeting) => println!("{}", meeting),
        };
    }

    Ok(())
}
