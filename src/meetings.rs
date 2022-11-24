use super::tokens::Tokens;
use chrono::DateTime;
use chrono::Local;
use reqwest::header;
use serde::ser::SerializeStruct;
use serde::Deserialize;
use serde::Serialize;
use std::error::Error;
use std::fmt::Display;

use regex::Regex;

fn calendar_url(email: &str, time_min: &str, time_max: &str) -> String {
    let time_min = urlencoding::encode(time_min).into_owned();
    let time_max = urlencoding::encode(time_max).into_owned();
    format!("https://www.googleapis.com/calendar/v3/calendars/{email}/events?timeMin={time_min}&timeMax={time_max}&singleEvents=true&showDeleted=false")
}

#[derive(Deserialize, Clone, Debug, Default)]
struct Attendee {
    #[serde(rename = "responseStatus")]
    response_status: String,
    #[serde(rename = "self")]
    #[serde(default)]
    is_self: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
struct MeetTime {
    #[serde(rename = "dateTime")]
    date_time: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Meeting {
    summary: Option<String>,
    start: Option<MeetTime>,
    end: Option<MeetTime>,
    #[serde(rename = "hangoutLink")]
    hangout_link: Option<String>,
    description: Option<String>,
    #[serde(default)]
    attendees: Vec<Attendee>,
}

#[derive(Debug, Serialize)]
struct FormattedDateTime {
    date: String,
    time: String,
}

fn extract_date_time(date_time: &Option<MeetTime>) -> Option<FormattedDateTime> {
    date_time
        .as_ref()
        .and_then(|d| DateTime::parse_from_rfc3339(&d.date_time).ok())
        .map(|d| FormattedDateTime {
            time: d.with_timezone(&Local).format("%H:%M").to_string(),
            date: d.with_timezone(&Local).format("%d/%m/%Y").to_string(),
        })
}

impl Serialize for Meeting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let start = extract_date_time(&self.start);
        let end = extract_date_time(&self.end);

        let mut s = serializer.serialize_struct("Meeting", 4)?;
        s.serialize_field("summary", &self.summary)?;
        s.serialize_field("start", &start)?;
        s.serialize_field("end", &end)?;
        s.serialize_field("hangoutLink", &self.hangout_link)?;
        s.end()
    }
}

impl Display for Meeting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let link = &self.get_link().unwrap_or("not present".to_string());
        let summary = &self.summary.clone().unwrap_or("No summary".to_string());
        let description = &self
            .description
            .clone()
            .unwrap_or("No description".to_string());

        write!(
            f,
            "{}\n{} - {}\nDescription: {}\nMeet: {}",
            summary,
            self.start()
                .map(|date| date.format("%H:%M").to_string())
                .unwrap_or("No start time".to_owned()),
            self.end()
                .map(|date| date.format("%H:%M").to_string())
                .unwrap_or("No end time".to_string()),
            description,
            link
        )
    }
}

impl Meeting {
    pub fn get_link(&self) -> Option<String> {
        let description_link = self.description.as_ref().and_then(|description| {
            let gather_link = Regex::new("https://app.gather.town[^\\s\"]*")
                .unwrap()
                .find(&description)
                .map(|m| m.as_str().into());

            let zoom_link = Regex::new("https://[^\\s\"]*zoom.us[^\\s\"]*")
                .unwrap()
                .find(&description)
                .map(|m| m.as_str().into());

            gather_link.or(zoom_link)
        });

        description_link.or_else(|| self.hangout_link.clone())
    }

    fn start(&self) -> Result<DateTime<Local>, Box<dyn Error>> {
        match &self.start {
            None => Err("No start time".into()),
            Some(start) => Ok(start.date_time.parse()?),
        }
    }

    fn end(&self) -> Result<DateTime<Local>, Box<dyn Error>> {
        match &self.end {
            None => Err("No end time".into()),
            Some(end) => Ok(end.date_time.parse()?),
        }
    }

    fn accepted(&self) -> bool {
        self.attendees
            .iter()
            .any(|attendee| attendee.is_self && attendee.response_status == "accepted")
    }
}

#[derive(Deserialize)]
struct Response {
    items: Vec<Meeting>,
}

fn retrieve_tokens() -> Result<Tokens, Box<dyn Error>> {
    Ok(Tokens::load()
        .or_else(|_| Tokens::do_login())?
        .refresh()
        .or_else(|_| Tokens::do_login())?)
}

async fn today_meetings_json(token: &str) -> Result<String, Box<dyn Error>> {
    let now = Local::now().date();
    let beginning_of_day = now.and_hms(0, 0, 0).to_rfc3339();
    let end_of_day = now.and_hms(23, 59, 59).to_rfc3339();

    let mut headers = header::HeaderMap::new();
    let token = format!("Bearer {token}");
    headers.insert("Authorization", header::HeaderValue::from_str(&token)?);

    let url = calendar_url(crate::config::EMAIL, &beginning_of_day, &end_of_day);
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;

    Ok(client.get(url).send().await?.text().await?)
}

async fn today_meetings(token: &str, debug: bool) -> Result<Response, Box<dyn Error>> {
    let response = today_meetings_json(&token).await?;
    if debug {
        println!("{}", response);
    }

    serde_json::from_str::<Response>(&response).map_err(Into::into)
}

fn next_meeting(meetings: &Vec<Meeting>, now: DateTime<Local>) -> Option<&Meeting> {
    meetings
        .into_iter()
        .filter(|meeting| {
            meeting.get_link().is_some()
                && meeting.start().is_ok()
                && meeting.end().map(|se| se > now).unwrap_or(false)
                && meeting.accepted()
        })
        .min_by_key(|meeting| {
            meeting
                .start()
                .map(|st| (st - now).num_seconds().abs())
                .unwrap()
        })
}

pub async fn retrieve(debug: bool) -> Result<Option<Meeting>, Box<dyn Error>> {
    let tokens = retrieve_tokens()?;

    retrieve_with_tokens(debug, tokens).await
}

pub async fn retrieve_with_tokens(
    debug: bool,
    tokens: Tokens,
) -> Result<Option<Meeting>, Box<dyn Error>> {
    let now = Local::now();

    let today_meetings = today_meetings(&tokens.access_token, debug).await?;
    let meeting = next_meeting(&today_meetings.items, now).cloned();
    Ok(meeting)
}

pub async fn json() -> Result<String, Box<dyn Error>> {
    let tokens = retrieve_tokens()?;
    let today_meetings = today_meetings_json(&tokens.access_token).await?;

    Ok(today_meetings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_link_gather_town() {
        let m = Meeting {
            description: Some(
                "This is on gather town! https://app.gather.town/meetings/XXXXX".to_string(),
            ),
            hangout_link: Some("https://meet.google.com/uq-q-q-q-q".to_string()),
            ..Default::default()
        };

        assert_eq!(
            m.get_link().unwrap(),
            "https://app.gather.town/meetings/XXXXX"
        );
    }

    #[test]
    fn gets_zoom_link() {
        let m = Meeting {
            description: Some("This is on zoom! https://us02web.zoom.us/j/88888888888".to_string()),
            hangout_link: Some("https://meet.google.com/uq-q-q-q-q".to_string()),
            ..Default::default()
        };

        assert_eq!(
            m.get_link().unwrap(),
            "https://us02web.zoom.us/j/88888888888"
        );
    }

    #[test]
    fn accepted_declined() {
        let m = Meeting {
            attendees: vec![Attendee {
                is_self: true,
                response_status: "declined".to_string(),
            }],
            ..Default::default()
        };

        assert!(!m.accepted());

        let m = Meeting {
            attendees: vec![Attendee {
                is_self: true,
                response_status: "pending".to_string(),
            }],
            ..Default::default()
        };

        assert!(!m.accepted());

        let m = Meeting {
            attendees: vec![Attendee {
                is_self: true,
                response_status: "accepted".to_string(),
            }],
            ..Default::default()
        };

        assert!(m.accepted());
    }
}
