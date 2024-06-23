use reqwest::Client;
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::env::args;
use std::time::SystemTime;

#[tokio::main]
async fn main() {
    let now = SystemTime::now();
    let args: Vec<String> = args().collect();
    //let url = &args[1].parse::<hyper::Uri>().unwrap();

    let client = Client::new();
    let origin_url = "https://monzo.com/";
    //Many websites are protected by Cloudflare, which detects if a request is coming from a real user or a web scraper (that's us!).
    // If it detects that the request is coming from a web scraper, it will block the request and return a 403 Forbidden status code.
    // To avoid this, we can set the User-Agent header to a value that is commonly used by web browsers.
    // Interestingly, when I started writing this programme in python I didn't run into this problem - so the python requests library must be handling this under the covers!
    let mut res = client.get(origin_url).header("User-Agent", "Mozilla/5.0 (iPad; CPU OS 12_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148").send().await.unwrap();
    println!("Status for {}: {}", origin_url, res.status());
    let mut crawler = Crawler::new(origin_url.to_string());
    crawler.crawl().await;
    match now.elapsed() {
        Ok(elapsed) => {
            // it prints '2'
            println!("{}", elapsed.as_secs());
        }
        Err(e) => {
            // an error occurred!
            println!("Error: {e:?}");
        }
    }
}

struct Crawler {
    //Note don't need to wrap client in an Arc as the Client type already uses an Arc internally, therefore can be safely shared between threads
    client: Client,
    urls_to_visit: Vec<String>,
    urls: HashMap<String, Vec<String>>,
}

impl Crawler {
    fn new(root: String) -> Crawler {
        Crawler {
            client: Client::new(),
            urls_to_visit: vec![root],
            urls: HashMap::new(),
        }
    }

    async fn crawl(&mut self) {
        while !self.urls_to_visit.is_empty() {
            let path = self.urls_to_visit.pop().unwrap();
            let res = self.client.get(path.clone()).header("User-Agent", "Mozilla/5.0 (iPad; CPU OS 12_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148").send().await.unwrap();
            let text = res.text().await.unwrap();
            let document = Html::parse_document(&text);
            // The a tag in html is used to define a hyperlink between from one page to another
            let selector = Selector::parse("a").unwrap();
            for a_tag in document.select(&selector) {
                let url = match a_tag.value().attr("href") {
                    Some(url) => url.to_string(),
                    None => continue,
                };
                if url.starts_with("https://monzo.com") {
                    if !self.urls.contains_key(&url) {
                        self.urls_to_visit.push(url.clone());
                    }
                    self.urls.entry(path.clone()).or_insert(vec![]).push(url);
                }
            }
        }

        println!("{:#?}", self.urls);
        for url in self.urls_to_visit.iter() {
            println!("{}", url);
        }
    }
}
