use reqwest::{Client, Response};
use scraper::{Html, Selector};
use std::collections::HashMap;
use std::env::args;
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, SystemTime};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() {
    // Mark the start time of the crawl, so we can measure how long it takes.
    let now = SystemTime::now();
    let args: Vec<String> = args().collect();
    if args.len() != 2 {
        println!("Input error: Incorrect number of arguments provided, precisely one argument should be given.");
        exit(1)
    }
    let origin_url = &args[1];

    let client = Client::new();
    // Send GET request to check if server is returning 200s.
    // Many websites are protected by Cloudflare, which detects if a request is coming from a real user or a web scraper (that's us!).
    // If it detects that the request is coming from a web scraper, it will block the request and return a 403 Forbidden status code.
    // To avoid this, we can set the User-Agent header to a value that is commonly used by web browsers.
    // Interestingly, when I started writing this programme in python I didn't run into this problem - so the python requests library must be handling this under the covers!
    let res = client.get(origin_url).header("User-Agent", "Mozilla/5.0 (iPad; CPU OS 12_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148").send().await.unwrap();
    println!("Status for {}: {}", origin_url, res.status());
    if res.status() == reqwest::StatusCode::ACCEPTED {
        println!("Target server is under load and returning 202s which we don't currently handle. Retry shortly.")
    }

    let crawler = Crawler::new(origin_url.to_string());
    // Crawl the server.
    crawler.crawl_whole().await;

    // Print out how long the crawler took.
    println!(
        "{:#?}",
        now.elapsed()
            .expect("Error calculating time taken to crawl.")
            .as_secs()
    )
}

struct Crawler {
    // The original URL passed in as an argument to cargo run
    root: String,
    // An HTTP client, to be reused for each GET request.
    //Note don't need to wrap client in an Arc as the Client type already uses an Arc internally, therefore can be safely shared between threads
    client: Client,
    // A store of outstanding URLs that need to be crawled
    urls_to_visit: Arc<Mutex<Vec<String>>>,
    // A key value store of all URLs found on the page of a given URL
    urls: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl Crawler {
    fn new(root: String) -> Crawler {
        Crawler {
            root: root.clone(),
            client: Client::new(),
            urls_to_visit: Arc::new(Mutex::new(vec![root])),
            urls: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// This function is responsible for crawling the whole website. It does this by spawning a number of tasks, each of which crawls an individual URL.
    /// The function uses a while loop to repeatedly spawn tasks until there are no more URLs to visit.
    /// It uses a JoinSet to wait for all the tasks to complete.
    /// It uses a semaphore to limit the number of concurrent tasks that are spawned to avoid hitting the maximum number of open sockets.
    /// It prints out a map, with each URL as a key, and the value as a list of URLs that it links to.
    async fn crawl_whole(self) {
        // There's an upper limit of how many outgoing TCP connections we can open at a given time (limited by how many sockets we can open)- if we try and spawn
        // more tokio tasks than this limit, we'll hit an error for having too many files open. Therefore, we limit the number of concurrent tasks spawned
        // using a semaphore. The semaphore is initialized with the number of concurrent tasks we want to allow, and each time we spawn a task, we acquire a permit
        let sem = Arc::new(Semaphore::new(100));

        // JoinSet is a helper struct that allows us to spawn a number of tasks and then wait for them all to complete.
        let mut set = JoinSet::new();
        // Spawn an "initial task" that will sleep for 1 second. This is necessary so that we can initially enter the while loop immediately below.
        set.spawn(async {
            sleep(Duration::from_secs(1));
        });

        // Enter a while loop that will continue until there are no more URLs to visit. For that to be true, the `urls_to_visit` vector must be empty and
        // all the tasks spawned must have completed. Use the `join_next` method on the JoinSet to check if there are any tasks that have not yet completed.
        while set.join_next().await.is_some() || !self.urls_to_visit.lock().unwrap().is_empty() {
            while !self
                .urls_to_visit
                .lock()
                .expect("Count not obtain lock")
                .is_empty()
            {
                // Aquire a permit from the semaphore, which will block if the number of concurrent tasks has reached the limit.
                let permit = Arc::clone(&sem).acquire_owned().await;

                // Pop off a URL from the urls_to_visit
                let path = self
                    .urls_to_visit
                    .clone()
                    .lock()
                    .expect("Count not obtain lock")
                    .pop()
                    .unwrap();

                // Clone the necessary variables so that they can be moved into the spawned task.
                let urls_to_visit = self.urls_to_visit.clone();
                let urls = self.urls.clone();
                let client = self.client.clone();
                let root = self.root.clone();

                // Spawn a task to crawl the given URL
                set.spawn(async move {
                    // Obtain a permit - this will block if we've reached the upper limit of how many concurrent tasks we can have
                    let _permit = permit;
                    crawl_individual_url(path, client.clone(), urls, urls_to_visit, root.clone())
                        .await;
                });
            }
        }
        println!("urls {:#?}", self.urls.lock().unwrap());
        println!("length of urls {:#?}", self.urls.lock().unwrap().len());
    }
}

/// This function is responsible for crawling an individual URL. It sends a GET request to the URL, and then parses the HTML response.
/// It then extracts all the URLs from the HTML response, and adds them to the `urls_to_visit` vector if they are not already present.
/// It also adds the URL to the `urls` hashmap, which maps a URL to all the URLs that it links to.
async fn crawl_individual_url(
    path: String,
    client: Client,
    urls: Arc<Mutex<HashMap<String, Vec<String>>>>,
    urls_to_visit: Arc<Mutex<Vec<String>>>,
    root: String,
) {
    // If the path given is relative to the root (ie it starts with "/"), prepend it with the root
    let url = if path.starts_with("/") {
        root.clone() + &path
    } else {
        path.clone()
    };

    // Make get request to provided URL
    let res = match client.get(url.clone()).header("User-Agent", "Mozilla/5.0 (iPad; CPU OS 12_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E147").send().await {
        Ok(res) => res,
        Err(e) => {
            println!("Error: {:#?}", e);
            return;
        }
    };
    // If the status code is not 200, return early
    match res.status() {
        reqwest::StatusCode::OK => (),
        _ => return,
    }
    // Extract the URLs from the HTML of the response
    let extracted_urls = parse_resp_to_urls(res).await;

    for child_url in extracted_urls.iter() {
        // Filter for URLs that have the same domain name as the root URL passed in

        if child_url.starts_with(&root) || child_url.starts_with("/") {
            // Add the URL to the urls_to_visit object, unless its already present
            if !urls
                .lock()
                .expect("Could not obtain lock")
                .contains_key(child_url)
            {
                urls_to_visit
                    .lock()
                    .expect("Could not obtain lock")
                    .push(child_url.clone());
            }
            // Store the URL as a child URL for the parent URL in the urls object
            urls.lock()
                .expect("Could not obtain lock")
                .entry(path.clone())
                .or_insert(vec![])
                .push(child_url.to_owned());
        }
    }
}

/// Takes a GET response, extracts the HTML from the text, filters for a tags (where hyperlinks are specified in HTML),
/// filters for those which contain hrefs, and returns a vector of URLs as strings.
async fn parse_resp_to_urls<'a>(res: Response) -> Vec<String> {
    let mut urls = vec![];
    let text = res.text().await.unwrap();
    let document = Html::parse_document(&text);
    // Filter for HTML a tags, which define child hyperlinks
    let selector = Selector::parse("a").unwrap();
    for a_tag in document.select(&selector) {
        let url = match a_tag.value().attr("href") {
            Some(url) => url.to_string(),
            None => continue,
        };
        urls.push(url);
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    /// Test that given a single URL, the correct child URLs are appended to the urls_to_visit and urls objects correctly.
    /// Note for this test and all others, we use the test website scrapethissite.com
    async fn test_single_url_crawl() {
        let client = Client::new();
        let path = "https://www.scrapethissite.com/".to_string();
        let urls = Arc::new(Mutex::new(HashMap::new()));
        let urls_to_visit = Arc::new(Mutex::new(vec![]));
        let root = "https://www.scrapethissite.com/".to_string();
        crawl_individual_url(path, client, urls.clone(), urls_to_visit.clone(), root).await;
        let expected = vec![
            "/".to_string(),
            "/pages/".to_string(),
            "/lessons/".to_string(),
            "/faq/".to_string(),
            "/login/".to_string(),
            "/pages/".to_string(),
            "/lessons/".to_string(),
        ];
        assert_eq!(
            *urls_to_visit.lock().expect("couldn't obtain lock"),
            expected.clone(),
        );
        assert_eq!(
            *urls.lock().expect("couldn't obtain lock")["https://www.scrapethissite.com/"],
            expected
        );
    }

    #[tokio::test]
    /// Test that for a given webpage, we successfully extract the hrefs.
    async fn test_html_parsing() {
        let expected = vec![
            "/".to_string(),
            "/pages/".to_string(),
            "/lessons/".to_string(),
            "/faq/".to_string(),
            "/login/".to_string(),
            "/pages/".to_string(),
            "/lessons/".to_string(),
        ];
        let client = Client::new();
        let path = "https://www.scrapethissite.com/".to_string();
        let res =  client.get(path.clone()).header("User-Agent", "Mozilla/5.0 (iPad; CPU OS 12_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E147").send().await.unwrap();
        let ans = parse_resp_to_urls(res).await;
        assert_eq!(ans, expected);
    }
}
