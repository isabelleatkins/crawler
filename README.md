# Overview
This is a Rust implementation of a scraper which, given a root url of format https://<domainname>, prints a HashMap where the key is a URL, and the value is all linked URLs from that URL page.

# Implementation details
It starts with a root URL. It sends a GET request to that URL, and parses the HTML from the response, extracting all linked URLs from the webpage. It checks if those links are either relative paths (starting "/"), or share the domain prefix - if so, it stores these child URLs in the hashmap entry for the root URL. It also stores each URL in a separate vector, which stores all URLs which have not yet been visited.
It iterates through this vector of urls_to_visit, and recursively does the above on each entry. It does this repeatedly, until all URLs have been visited.

To improve the speed of the programme, I spawned new tokio tasks each time we want to crawl over a new URL. I needed to set a maximum for how many tasks were spawned to avoid hitting an error when attempting to open too many TCP connections at the same time. I set this value to 100- exploring what it should be exactly is identified in the Areas for Improvement section below.

I edited the headers of the GET request to get around Cloudflare's blocking of requests that look like they come from crawlers- otherwise, we'd get 403s for our GET requests. I did this by setting the User Agent to something that looked like a "real user".

Hyperlinks in the HTML could be of two forms: absolute paths or relative paths. I've added support for both. Note, this crawler only explores paths which have the same subdomain.


# How to use
1. Clone the repository
1. From the root of the repository, set your 
```console
cargo run `target_url`
```

e.g 

```console
cargo run https://google.com
```

# Areas for improvement

This was a timeboxed exercise and I prioritised making the crate more performant by adding concurrency over a few other items. I've listed those items below, which should be improved.

- Unique vector values in the value of a HashMap: I didn't get a chance to make the child URLs stored in the HashMap value were unique. This should be done by eg turning the vector into a HashSet.

- Handling 202s: A 202 tells us the request is accepted, but is in a queue to be processed. Currently, if we receive a 202, we move on, and that URL is not written into our HashMap. That might mean some entries are missed (best case), or worst case: if we receive a 202 for the *first* GET request for the root URL, then the programme will return immediately! So no other URLs are visited and our return result is empty. Currently we print a warning message if this worst case is hit, so that the user knows to retry shortly. The user can rerun manually as a work-around. Nevertheless, this should be improved on. Perhaps for instance, we should check if the location field in the 202 response headers points to the location we'll be able to find the content soon. Or perhaps we retry.

- Refactor for better encapsulation. I don't like that crawl_individual_url is a free function that accesses/modifies the Crawler struct's state - that's bad encapsulation. I did this as a work around: when trying to spawn threads which directly accessed state by 'self.x', I was getting Compiler complaints about Self not being Copy and thread safe. This needs to be improved upon.

- Error handling: Currently, there's a bunch of .unwrap() calls. Most of these are safe (ie we know we'll only get an Ok() or a Some()) but I haven't checked all of them. If I had more time, I'd check them all and improve the error handling. I don't want one failed GET request to a given URL to cause the whole programme to fail. Instead, it prints the error hit, and continues. This would result in an incomplete set. 

- I used a synchronous Mutex. This blocks threads for a small amount of time whilst the lock is held. This is fine and standard, on the assumption that lock contention isn't too high. I haven't played around with what "too high" looks like, but this crate spawns 100 concurrent threads trying to obtain the lock. To improve on this, investigate how many threads we should spawn to optimise speed whilst not raising lock contention.

- More testing: I added some noddy tests, but the coverage is lacking.
# Code iterations and resulting time improvements
1. For the first pass, I wrote synchronous single-threaded code which would check each URL in turn.
    time taken: 306s

1. For a second pass, I spawned a tokio task for each URL in the vector of urls_to_visit, so that they could be searched concurrently. I wrapped the shared state objects in Arc<Mutexes> so that they could be shared across threads safely.
    time taken: 38s

2. For a third pass, I realised I was missing any relative paths - so I added support for relative paths, which meant our net was catching about five 6 times the number of paths. So the time went up.
    time taken: 59s

