use anyhow::Result;
use clap::Parser;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// Custom error types for better error handling
#[derive(Error, Debug)]
pub enum ScraperError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Invalid CSS selector: {0}")]
    InvalidSelector(String),
    #[error("Timeout: Request took longer than {0} seconds")]
    Timeout(u64),
    #[error("Crawl depth exceeded maximum: {0}")]
    DepthExceeded(usize),
    #[error("HTTP {0}: {1}")]
    HttpStatus(u16, String),
    #[error("Anti-bot protection detected: {0}")]
    AntiBotDetected(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
}

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "simple-web-scraper")]
#[command(about = "A simple but powerful web scraper", long_about = None)]
struct Args {
    /// URL(s) to scrape (can provide multiple, or use --url-file)
    urls: Vec<String>,

    /// Output format: json, csv, or text
    #[arg(short, long, default_value = "json")]
    format: String,

    /// Request timeout in seconds
    #[arg(short, long, default_value = "30")]
    timeout: u64,

    /// Custom user agent
    #[arg(short, long)]
    user_agent: Option<String>,

    /// Proxy URL (e.g., http://proxy.example.com:8080)
    #[arg(short, long)]
    proxy: Option<String>,

    /// Custom CSS selector to extract (can specify multiple)
    #[arg(short, long)]
    selector: Vec<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Quiet mode (minimal output)
    #[arg(short, long)]
    quiet: bool,

    /// Delay between requests in milliseconds
    #[arg(short, long, default_value = "1000")]
    delay: u64,

    /// Enable crawling (follow links)
    #[arg(long)]
    crawl: bool,

    /// Maximum crawl depth
    #[arg(long, default_value = "2")]
    max_depth: usize,

    /// Maximum number of pages to crawl
    #[arg(long, default_value = "10")]
    max_pages: usize,

    /// Allow crawling to specific domains (comma-separated, e.g., "example.com,docs.example.com")
    #[arg(long)]
    allow_domains: Option<String>,

    /// Block crawling to specific domains (comma-separated, e.g., "ads.example.com,tracker.com")
    #[arg(long)]
    block_domains: Option<String>,

    /// Enable cross-domain crawling (follow links to any domain)
    #[arg(long)]
    cross_domain: bool,

    /// Extract metadata (Open Graph, meta tags)
    #[arg(long)]
    metadata: bool,

    /// Save output to file
    #[arg(short, long)]
    output: Option<String>,

    /// Read URLs from a file (one URL per line)
    #[arg(long)]
    url_file: Option<String>,

    /// Save each scraped page to a separate file (requires --output as prefix)
    #[arg(long)]
    output_per_page: bool,
}

/// Metadata extracted from the page
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Metadata {
    description: Option<String>,
    keywords: Option<String>,
    author: Option<String>,
    og_title: Option<String>,
    og_description: Option<String>,
    og_image: Option<String>,
    og_url: Option<String>,
    canonical_url: Option<String>,
    favicon: Option<String>,
}

/// Custom selector result
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CustomSelectorResult {
    selector: String,
    matches: Vec<String>,
}

/// Main scraped data structure
#[derive(Debug, Serialize, Deserialize, Clone)]
struct ScrapedData {
    url: String,
    status_code: u16,
    title: Option<String>,
    headings: Vec<String>,
    paragraphs: Vec<String>,
    links: Vec<Link>,
    images: Vec<Image>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tables: Vec<Table>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    code_blocks: Vec<CodeBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Metadata>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    custom_selectors: Vec<CustomSelectorResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    depth: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Link {
    text: String,
    url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Image {
    alt: String,
    src: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CodeBlock {
    content: String,
    language: Option<String>,
}

// ========== Helper Functions for Testability ==========

/// Normalize a URL to absolute form
/// Returns None if the URL cannot be normalized
fn normalize_url(base_url: &Url, relative_url: &str) -> Option<String> {
    if relative_url.starts_with("http://") || relative_url.starts_with("https://") {
        Some(relative_url.to_string())
    } else if relative_url.starts_with("//") {
        Some(format!("https:{}", relative_url))
    } else {
        base_url.join(relative_url).ok().map(|u| u.to_string())
    }
}

/// Check if a URL belongs to the same domain as the base domain
fn is_same_domain(url: &str, base_domain: &str) -> bool {
    if let Ok(parsed_url) = Url::parse(url) {
        parsed_url.domain() == Some(base_domain)
    } else {
        false
    }
}

/// Read URLs from a file (one URL per line)
/// Skips empty lines and lines starting with #
fn read_urls_from_file(file_path: &str) -> Result<Vec<String>> {
    let file = fs::File::open(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to open URL file '{}': {}", file_path, e))?;

    let reader = BufReader::new(file);
    let mut urls = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| {
            anyhow::anyhow!("Failed to read line {} from '{}': {}", line_num + 1, file_path, e)
        })?;

        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Validate URL
        if let Err(e) = Url::parse(trimmed) {
            log::warn!(
                "Skipping invalid URL on line {} in '{}': {} (error: {})",
                line_num + 1,
                file_path,
                trimmed,
                e
            );
            continue;
        }

        urls.push(trimmed.to_string());
    }

    if urls.is_empty() {
        return Err(anyhow::anyhow!(
            "No valid URLs found in file '{}'",
            file_path
        ));
    }

    log::info!("Loaded {} URL(s) from file '{}'", urls.len(), file_path);
    Ok(urls)
}

/// Classify HTTP status code and return a user-friendly error message
fn classify_http_status(status_code: u16, url: &str) -> Result<(), ScraperError> {
    match status_code {
        200..=299 => Ok(()),
        400 => Err(ScraperError::HttpStatus(
            400,
            format!("Bad Request - The server couldn't understand the request to {}", url),
        )),
        401 => Err(ScraperError::HttpStatus(
            401,
            format!("Unauthorized - Authentication required to access {}", url),
        )),
        403 => Err(ScraperError::HttpStatus(
            403,
            format!("Forbidden - Access denied to {}. This may indicate bot protection.", url),
        )),
        404 => Err(ScraperError::HttpStatus(
            404,
            format!("Not Found - The page {} does not exist", url),
        )),
        429 => Err(ScraperError::RateLimited(
            format!("Too many requests to {}. Please slow down and try again later.", url),
        )),
        500 => Err(ScraperError::HttpStatus(
            500,
            format!("Internal Server Error - The server at {} encountered an error", url),
        )),
        502 => Err(ScraperError::HttpStatus(
            502,
            format!("Bad Gateway - The server at {} received an invalid response", url),
        )),
        503 => Err(ScraperError::HttpStatus(
            503,
            format!("Service Unavailable - The server at {} is temporarily unavailable", url),
        )),
        504 => Err(ScraperError::HttpStatus(
            504,
            format!("Gateway Timeout - The server at {} took too long to respond", url),
        )),
        _ => Err(ScraperError::HttpStatus(
            status_code,
            format!("HTTP error {} while accessing {}", status_code, url),
        )),
    }
}

/// Detect common anti-bot protection patterns in HTML content
fn detect_anti_bot_features(html: &str, title: Option<&str>) -> Option<String> {
    // Check for Cloudflare challenge
    if html.contains("cf-browser-verification") || html.contains("Cloudflare") && html.contains("challenge-platform") {
        return Some("Cloudflare protection detected. The site is checking if you're a bot.".to_string());
    }

    // Check for Cloudflare Ray ID (common in error pages)
    if html.contains("Cloudflare Ray ID") || html.contains("cf-ray") {
        return Some("Cloudflare error page detected. Access may be restricted.".to_string());
    }

    // Check for reCAPTCHA
    if html.contains("recaptcha") || html.contains("g-recaptcha") {
        return Some("reCAPTCHA detected. Human verification required.".to_string());
    }

    // Check for hCaptcha
    if html.contains("hcaptcha") || html.contains("h-captcha") {
        return Some("hCaptcha detected. Human verification required.".to_string());
    }

    // Check for common bot detection services
    if html.contains("PerimeterX") || html.contains("px-captcha") {
        return Some("PerimeterX bot detection detected.".to_string());
    }

    // Check for DataDome
    if html.contains("datadome") || html.contains("DataDome") {
        return Some("DataDome bot protection detected.".to_string());
    }

    // Check for Akamai Bot Manager
    if html.contains("akamai") && (html.contains("bot") || html.contains("challenge")) {
        return Some("Akamai bot protection detected.".to_string());
    }

    // Check title for common access denied messages
    if let Some(title_text) = title {
        let title_lower = title_text.to_lowercase();
        if title_lower.contains("access denied")
            || title_lower.contains("blocked")
            || title_lower.contains("forbidden")
            || title_lower.contains("captcha") {
            return Some(format!("Access restriction detected: '{}'", title_text));
        }
    }

    // Check for "Just a moment" or similar Cloudflare messages
    if html.contains("Just a moment") || html.contains("Checking your browser") {
        return Some("Cloudflare JavaScript challenge detected.".to_string());
    }

    None
}

/// Extract and normalize links from an HTML document
fn extract_links(document: &Html, base_url: &Url) -> Vec<Link> {
    let a_selector = Selector::parse("a").unwrap();
    document
        .select(&a_selector)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            let text = el.text().collect::<String>().trim().to_string();
            let absolute_url = normalize_url(base_url, href)?;

            Some(Link {
                text: if text.is_empty() {
                    href.to_string()
                } else {
                    text
                },
                url: absolute_url,
            })
        })
        .collect()
}

/// Extract and normalize images from an HTML document
fn extract_images(document: &Html, base_url: &Url) -> Vec<Image> {
    let img_selector = Selector::parse("img").unwrap();
    document
        .select(&img_selector)
        .filter_map(|el| {
            let src = el.value().attr("src")?;
            let alt = el.value().attr("alt").unwrap_or("").to_string();
            let absolute_src = normalize_url(base_url, src)?;

            Some(Image {
                alt,
                src: absolute_src,
            })
        })
        .collect()
}

/// Extract title from an HTML document
fn extract_title(document: &Html) -> Option<String> {
    let title_selector = Selector::parse("title").unwrap();
    document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
}

/// Extract all headings (h1-h6) from an HTML document
fn extract_headings(document: &Html) -> Vec<String> {
    let mut headings = Vec::new();
    for tag in &["h1", "h2", "h3", "h4", "h5", "h6"] {
        let selector = Selector::parse(tag).unwrap();
        for element in document.select(&selector) {
            let text = element.text().collect::<String>().trim().to_string();
            if !text.is_empty() {
                headings.push(text);
            }
        }
    }
    headings
}

/// Extract all paragraphs from an HTML document
fn extract_paragraphs(document: &Html) -> Vec<String> {
    let p_selector = Selector::parse("p").unwrap();
    document
        .select(&p_selector)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Extract all tables from an HTML document
fn extract_tables(document: &Html) -> Vec<Table> {
    let table_selector = Selector::parse("table").unwrap();

    document
        .select(&table_selector)
        .filter_map(|table_elem| {
            // Create selectors for table elements
            let th_selector = Selector::parse("th").unwrap();
            let tr_selector = Selector::parse("tr").unwrap();
            let td_selector = Selector::parse("td").unwrap();

            // Create a new HTML document from the table element
            let table_html = Html::parse_fragment(&format!("<table>{}</table>", table_elem.inner_html()));

            // Extract all headers
            let headers: Vec<String> = table_html
                .select(&th_selector)
                .map(|th| th.text().collect::<String>().trim().to_string())
                .filter(|text| !text.is_empty())
                .collect();

            // Extract all rows containing td elements
            let rows: Vec<Vec<String>> = table_html
                .select(&tr_selector)
                .filter_map(|tr| {
                    let cells: Vec<String> = tr
                        .select(&td_selector)
                        .map(|td| td.text().collect::<String>().trim().to_string())
                        .collect();

                    if cells.is_empty() {
                        None
                    } else {
                        Some(cells)
                    }
                })
                .collect();

            // Only include tables that have headers or rows
            if headers.is_empty() && rows.is_empty() {
                None
            } else {
                Some(Table { headers, rows })
            }
        })
        .collect()
}

/// Extract all code blocks from an HTML document
fn extract_code_blocks(document: &Html) -> Vec<CodeBlock> {
    let mut code_blocks = Vec::new();

    // Extract <pre><code> blocks (common pattern)
    let pre_selector = Selector::parse("pre").unwrap();
    let code_selector = Selector::parse("code").unwrap();

    for pre in document.select(&pre_selector) {
        let pre_html = Html::parse_fragment(&pre.html());
        let code_elements: Vec<_> = pre_html.select(&code_selector).collect();

        if !code_elements.is_empty() {
            // <pre><code> pattern
            for code in code_elements {
                let content = code.text().collect::<String>();
                let language = code
                    .value()
                    .attr("class")
                    .and_then(|classes| {
                        // Extract language from class like "language-rust" or "lang-python"
                        classes
                            .split_whitespace()
                            .find(|c| c.starts_with("language-") || c.starts_with("lang-"))
                            .map(|c| {
                                c.strip_prefix("language-")
                                    .or_else(|| c.strip_prefix("lang-"))
                                    .unwrap_or(c)
                                    .to_string()
                            })
                    });

                if !content.trim().is_empty() {
                    code_blocks.push(CodeBlock { content, language });
                }
            }
        } else {
            // Just <pre> without <code>
            let content = pre.text().collect::<String>();
            if !content.trim().is_empty() {
                code_blocks.push(CodeBlock {
                    content,
                    language: None,
                });
            }
        }
    }

    // Extract standalone <code> elements (not inside <pre>)
    for code in document.select(&code_selector) {
        // Check if this code element is inside a pre tag
        let mut is_inside_pre = false;
        let mut current = code.parent();
        while let Some(parent) = current {
            if let Some(element) = parent.value().as_element() {
                if element.name() == "pre" {
                    is_inside_pre = true;
                    break;
                }
            }
            current = parent.parent();
        }

        if !is_inside_pre {
            let content = code.text().collect::<String>();
            if !content.trim().is_empty() {
                let language = code
                    .value()
                    .attr("class")
                    .and_then(|classes| {
                        classes
                            .split_whitespace()
                            .find(|c| c.starts_with("language-") || c.starts_with("lang-"))
                            .map(|c| {
                                c.strip_prefix("language-")
                                    .or_else(|| c.strip_prefix("lang-"))
                                    .unwrap_or(c)
                                    .to_string()
                            })
                    });

                code_blocks.push(CodeBlock { content, language });
            }
        }
    }

    code_blocks
}

/// Process custom CSS selectors and extract matching elements
fn process_custom_selectors(
    document: &Html,
    selectors: &[String],
) -> Result<Vec<CustomSelectorResult>> {
    let mut results = Vec::new();

    for selector_str in selectors {
        match Selector::parse(selector_str) {
            Ok(selector) => {
                let matches: Vec<String> = document
                    .select(&selector)
                    .map(|el| el.text().collect::<String>().trim().to_string())
                    .filter(|text| !text.is_empty())
                    .collect();

                log::debug!(
                    "Custom selector '{}' found {} matches",
                    selector_str,
                    matches.len()
                );

                results.push(CustomSelectorResult {
                    selector: selector_str.clone(),
                    matches,
                });
            }
            Err(e) => {
                log::error!("Invalid selector '{}': {}", selector_str, e);
                return Err(ScraperError::InvalidSelector(format!(
                    "{}: {}",
                    selector_str, e
                ))
                .into());
            }
        }
    }

    Ok(results)
}

/// Parse comma-separated domain list into HashSet
fn parse_domain_list(domains_str: &str) -> HashSet<String> {
    domains_str
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Determine if a link should be added to the crawl queue
/// Applies filtering in order: block list → allow list → cross-domain → same-domain fallback
fn should_add_to_crawl_queue(
    link_url: &str,
    base_url: &Url,
    base_domain: &str,
    visited: &HashSet<String>,
    allow_domains: &HashSet<String>,
    block_domains: &HashSet<String>,
    cross_domain: bool,
) -> Option<String> {
    // Parse URL (try absolute first, then relative)
    let parsed_url = if let Ok(url) = Url::parse(link_url) {
        url
    } else if let Ok(url) = base_url.join(link_url) {
        url
    } else {
        log::debug!("❌ Skipping invalid URL: {}", link_url);
        return None;
    };

    let url_str = parsed_url.to_string();

    // Skip if already visited
    if visited.contains(&url_str) {
        log::debug!("⏭️  Skipping already visited: {}", url_str);
        return None;
    }

    // Get the domain of the link
    let link_domain = match parsed_url.domain() {
        Some(domain) => domain.to_lowercase(),
        None => {
            log::debug!("❌ Skipping URL with no domain: {}", url_str);
            return None;
        }
    };

    // 1️⃣ Apply block list first
    if !block_domains.is_empty() && block_domains.contains(&link_domain) {
        log::debug!("🚫 Blocked domain: {} ({})", url_str, link_domain);
        return None;
    }

    // 2️⃣ Check allow list (if specified)
    if !allow_domains.is_empty() {
        // Base domain is always implicitly allowed
        if link_domain == base_domain || allow_domains.contains(&link_domain) {
            log::debug!("✅ Allowed domain: {} ({})", url_str, link_domain);
            return Some(url_str);
        } else {
            log::debug!("⛔ Not in allow list: {} ({})", url_str, link_domain);
            return None;
        }
    }

    // 3️⃣ Check cross-domain flag
    if cross_domain {
        log::debug!("🌐 Cross-domain enabled: {} ({})", url_str, link_domain);
        return Some(url_str);
    }

    // 4️⃣ Fallback: same-domain only (default behavior)
    if link_domain == base_domain {
        log::debug!("🏠 Same domain: {} ({})", url_str, link_domain);
        return Some(url_str);
    } else {
        log::debug!("🔒 Different domain blocked: {} ({})", url_str, link_domain);
        return None;
    }
}

// ========== Main Application Logic ==========

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();

    // Initialize logger
    let log_level = if args.verbose {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    log::info!("🚀 Simple Web Scraper v0.2.0");

    // Load URLs from file if provided
    if let Some(ref url_file) = args.url_file {
        let file_urls = read_urls_from_file(url_file)?;
        args.urls.extend(file_urls);
    }

    // Validate that we have at least one URL
    if args.urls.is_empty() {
        return Err(anyhow::anyhow!(
            "No URLs provided. Use positional arguments or --url-file to specify URLs."
        ));
    }

    // Validate output-per-page option
    if args.output_per_page && args.output.is_none() {
        return Err(anyhow::anyhow!(
            "--output-per-page requires --output to be specified as a filename prefix"
        ));
    }

    log::info!("📋 Scraping {} URL(s)", args.urls.len());

    // Validate URLs
    for url in &args.urls {
        if let Err(e) = Url::parse(url) {
            return Err(ScraperError::InvalidUrl(format!("{}: {}", url, e)).into());
        }
    }

    // Scrape URLs
    let results = if args.crawl {
        // Crawl mode: follow links from the first URL
        if args.urls.len() > 1 {
            log::warn!("Crawl mode only uses the first URL provided");
        }
        crawl_website(&args).await?
    } else {
        // Regular mode: scrape provided URLs
        scrape_multiple(&args).await?
    };

    // Output results
    output_results(&results, &args)?;

    log::info!("✅ Scraped {} page(s) successfully", results.len());
    Ok(())
}

/// Scrape multiple URLs (non-crawling mode)
async fn scrape_multiple(args: &Args) -> Result<Vec<ScrapedData>> {
    let mut results = Vec::new();

    for url in &args.urls {
        log::info!("Scraping: {}", url);

        match scrape_website(url, args, None).await {
            Ok(data) => results.push(data),
            Err(e) => {
                log::error!("Failed to scrape {}: {}", url, e);
                if !args.quiet {
                    eprintln!("Error scraping {}: {}", url, e);
                }
            }
        }

        // Rate limiting delay
        if results.len() < args.urls.len() {
            log::debug!("Waiting {}ms before next request", args.delay);
            tokio::time::sleep(Duration::from_millis(args.delay)).await;
        }
    }

    Ok(results)
}

/// Crawl website following links
async fn crawl_website(args: &Args) -> Result<Vec<ScrapedData>> {
    let start_url = &args.urls[0];
    let base_url = Url::parse(start_url)?;
    let base_domain = base_url.domain().ok_or_else(|| {
        ScraperError::InvalidUrl("URL has no domain".to_string())
    })?;

    // Parse domain filtering lists
    let allow_domains = args
        .allow_domains
        .as_ref()
        .map(|s| parse_domain_list(s))
        .unwrap_or_default();
    let block_domains = args
        .block_domains
        .as_ref()
        .map(|s| parse_domain_list(s))
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((start_url.clone(), 0usize));

    log::info!("🕷️  Starting crawl from: {}", start_url);
    log::info!("📊 Max depth: {}, Max pages: {}", args.max_depth, args.max_pages);

    // Log domain filtering configuration
    if !allow_domains.is_empty() {
        log::info!("✅ Allow domains: {:?}", allow_domains);
    }
    if !block_domains.is_empty() {
        log::info!("🚫 Block domains: {:?}", block_domains);
    }
    if args.cross_domain {
        log::info!("🌐 Cross-domain crawling enabled");
    } else if allow_domains.is_empty() && block_domains.is_empty() {
        log::info!("🏠 Same-domain only (default)");
    }

    while let Some((url, depth)) = queue.pop_front() {
        if visited.contains(&url) || results.len() >= args.max_pages {
            continue;
        }

        if depth > args.max_depth {
            log::debug!("Skipping {} (depth {} > max {})", url, depth, args.max_depth);
            continue;
        }

        visited.insert(url.clone());
        log::info!("Crawling: {} (depth: {})", url, depth);

        match scrape_website(&url, args, Some(depth)).await {
            Ok(data) => {
                // Extract links for further crawling
                if depth < args.max_depth {
                    for link in &data.links {
                        if let Some(link_str) = should_add_to_crawl_queue(
                            &link.url,
                            &base_url,
                            base_domain,
                            &visited,
                            &allow_domains,
                            &block_domains,
                            args.cross_domain,
                        ) {
                            queue.push_back((link_str, depth + 1));
                        }
                    }
                }

                results.push(data);
            }
            Err(e) => {
                log::error!("Failed to crawl {}: {}", url, e);
            }
        }

        // Rate limiting
        tokio::time::sleep(Duration::from_millis(args.delay)).await;
    }

    Ok(results)
}

/// Scrape a single website
async fn scrape_website(url: &str, args: &Args, depth: Option<usize>) -> Result<ScrapedData> {
    log::debug!("Fetching: {}", url);

    // Build HTTP client with custom configuration
    let mut client_builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .user_agent(
            args.user_agent
                .as_deref()
                .unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"),
        );

    // Add proxy if specified
    if let Some(proxy_url) = &args.proxy {
        log::debug!("Using proxy: {}", proxy_url);
        client_builder = client_builder.proxy(reqwest::Proxy::all(proxy_url)?);
    }

    let client = client_builder.build().map_err(|e| {
        ScraperError::NetworkError(format!("Failed to build HTTP client: {}", e))
    })?;

    // Fetch the page with enhanced error handling
    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            ScraperError::Timeout(args.timeout)
        } else if e.is_connect() {
            ScraperError::NetworkError(format!("Connection failed to {}: {}", url, e))
        } else if e.is_request() {
            ScraperError::NetworkError(format!("Request error for {}: {}", url, e))
        } else {
            ScraperError::HttpError(e)
        }
    })?;

    let status_code = response.status().as_u16();

    // Check HTTP status code and provide detailed error messages
    classify_http_status(status_code, url)?;

    let html = response.text().await.map_err(|e| {
        ScraperError::NetworkError(format!("Failed to read response body from {}: {}", url, e))
    })?;

    let document = Html::parse_document(&html);
    let base_url = Url::parse(url)?;

    // Extract content using helper functions
    let title = extract_title(&document);

    // Detect anti-bot protection features
    if let Some(anti_bot_msg) = detect_anti_bot_features(&html, title.as_deref()) {
        log::warn!("Anti-bot detection for {}: {}", url, anti_bot_msg);
        return Err(ScraperError::AntiBotDetected(anti_bot_msg).into());
    }
    let headings = extract_headings(&document);
    let paragraphs = extract_paragraphs(&document);
    let links = extract_links(&document, &base_url);
    let images = extract_images(&document, &base_url);
    let tables = extract_tables(&document);
    let code_blocks = extract_code_blocks(&document);

    // Extract metadata if requested
    let metadata = if args.metadata {
        Some(extract_metadata(&document))
    } else {
        None
    };

    // Process custom selectors if provided
    let custom_selectors = process_custom_selectors(&document, &args.selector)?;

    Ok(ScrapedData {
        url: url.to_string(),
        status_code,
        title,
        headings,
        paragraphs,
        links,
        images,
        tables,
        code_blocks,
        metadata,
        custom_selectors,
        depth,
    })
}

/// Extract metadata from the HTML document
fn extract_metadata(document: &Html) -> Metadata {
    let meta_selector = Selector::parse("meta").unwrap();
    let link_selector = Selector::parse("link").unwrap();

    let mut metadata = Metadata {
        description: None,
        keywords: None,
        author: None,
        og_title: None,
        og_description: None,
        og_image: None,
        og_url: None,
        canonical_url: None,
        favicon: None,
    };

    // Extract meta tags
    for element in document.select(&meta_selector) {
        let name = element.value().attr("name").or_else(|| element.value().attr("property"));
        let content = element.value().attr("content");

        if let (Some(name), Some(content)) = (name, content) {
            match name.to_lowercase().as_str() {
                "description" => metadata.description = Some(content.to_string()),
                "keywords" => metadata.keywords = Some(content.to_string()),
                "author" => metadata.author = Some(content.to_string()),
                "og:title" => metadata.og_title = Some(content.to_string()),
                "og:description" => metadata.og_description = Some(content.to_string()),
                "og:image" => metadata.og_image = Some(content.to_string()),
                "og:url" => metadata.og_url = Some(content.to_string()),
                _ => {}
            }
        }
    }

    // Extract canonical URL and favicon
    for element in document.select(&link_selector) {
        let rel = element.value().attr("rel");
        let href = element.value().attr("href");

        if let (Some(rel), Some(href)) = (rel, href) {
            match rel.to_lowercase().as_str() {
                "canonical" => metadata.canonical_url = Some(href.to_string()),
                "icon" | "shortcut icon" => metadata.favicon = Some(href.to_string()),
                _ => {}
            }
        }
    }

    metadata
}

/// Output results in the requested format
fn output_results(results: &[ScrapedData], args: &Args) -> Result<()> {
    // Handle per-page output mode
    if args.output_per_page {
        // Validation in main() ensures args.output is Some when output_per_page is true
        let output_prefix = args.output.as_ref().unwrap();

        // Determine file extension based on format
        let extension = match args.format.to_lowercase().as_str() {
            "json" => "json",
            "csv" => "csv",
            "text" | "txt" => "txt",
            other => {
                log::error!("Unknown format: {}", other);
                return Err(anyhow::anyhow!(
                    "Unknown format '{}'. Use: json, csv, or text",
                    other
                ));
            }
        };

        log::info!("💾 Writing {} pages to individual files with prefix '{}'", results.len(), output_prefix);

        // Write each result to a separate file
        for (index, data) in results.iter().enumerate() {
            let filename = format!("{}_{:03}.{}", output_prefix, index + 1, extension);

            // Format single result
            let output_str = match args.format.to_lowercase().as_str() {
                "json" => format_json(&[data.clone()])?,
                "csv" => format_csv(&[data.clone()])?,
                "text" | "txt" => format_text(&[data.clone()]),
                _ => unreachable!(), // Already validated above
            };

            std::fs::write(&filename, &output_str)?;
            log::info!("  ✓ Saved: {}", filename);
        }

        log::info!("✅ All {} pages saved successfully", results.len());
        return Ok(());
    }

    // Standard output mode - all results in one file/stdout
    let output_str = match args.format.to_lowercase().as_str() {
        "json" => format_json(results)?,
        "csv" => format_csv(results)?,
        "text" | "txt" => format_text(results),
        other => {
            log::error!("Unknown format: {}", other);
            return Err(anyhow::anyhow!(
                "Unknown format '{}'. Use: json, csv, or text",
                other
            ));
        }
    };

    // Write to file or stdout
    if let Some(output_file) = &args.output {
        std::fs::write(output_file, &output_str)?;
        log::info!("💾 Output saved to: {}", output_file);
    } else if !args.quiet {
        println!("{}", output_str);
    }

    Ok(())
}

/// Format results as JSON
fn format_json(results: &[ScrapedData]) -> Result<String> {
    Ok(serde_json::to_string_pretty(results)?)
}

/// Format results as CSV
fn format_csv(results: &[ScrapedData]) -> Result<String> {
    let mut writer = csv::Writer::from_writer(vec![]);

    // Write header
    writer.write_record(&[
        "url",
        "status_code",
        "title",
        "headings_count",
        "paragraphs_count",
        "links_count",
        "images_count",
        "tables_count",
        "code_blocks_count",
        "depth",
    ])?;

    // Write data rows
    for data in results {
        writer.write_record(&[
            &data.url,
            &data.status_code.to_string(),
            &data.title.clone().unwrap_or_default(),
            &data.headings.len().to_string(),
            &data.paragraphs.len().to_string(),
            &data.links.len().to_string(),
            &data.images.len().to_string(),
            &data.tables.len().to_string(),
            &data.code_blocks.len().to_string(),
            &data.depth.map(|d| d.to_string()).unwrap_or_default(),
        ])?;
    }

    Ok(String::from_utf8(writer.into_inner()?)?)
}

/// Truncate text to a maximum length with ellipsis
fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", &text[..max_len])
    } else {
        text.to_string()
    }
}

/// Format a list with a preview limit
fn format_text_list<F>(
    output: &mut String,
    title: &str,
    items: &[String],
    preview_limit: usize,
    format_fn: F,
) where
    F: Fn(&str) -> String,
{
    if items.is_empty() {
        return;
    }

    output.push_str(&format!("\n{} ({}):\n", title, items.len()));
    for item in items.iter().take(preview_limit) {
        output.push_str(&format_fn(item));
    }
    if items.len() > preview_limit {
        output.push_str(&format!(
            "  ... and {} more\n",
            items.len() - preview_limit
        ));
    }
}

/// Format metadata section for text output
fn format_text_metadata(metadata: &Metadata) -> String {
    let mut output = String::from("\nMetadata:\n");

    if let Some(desc) = &metadata.description {
        output.push_str(&format!("  Description: {}\n", desc));
    }
    if let Some(keywords) = &metadata.keywords {
        output.push_str(&format!("  Keywords: {}\n", keywords));
    }
    if let Some(author) = &metadata.author {
        output.push_str(&format!("  Author: {}\n", author));
    }
    if let Some(og_title) = &metadata.og_title {
        output.push_str(&format!("  OG Title: {}\n", og_title));
    }
    if let Some(og_image) = &metadata.og_image {
        output.push_str(&format!("  OG Image: {}\n", og_image));
    }

    output
}

/// Format custom selectors section for text output
fn format_text_custom_selectors(custom_selectors: &[CustomSelectorResult]) -> String {
    let mut output = String::from("\nCustom Selectors:\n");

    for result in custom_selectors {
        output.push_str(&format!(
            "  '{}' ({} matches):\n",
            result.selector,
            result.matches.len()
        ));
        for (i, match_text) in result.matches.iter().take(3).enumerate() {
            output.push_str(&format!("    {}. {}\n", i + 1, match_text));
        }
        if result.matches.len() > 3 {
            output.push_str(&format!(
                "    ... and {} more\n",
                result.matches.len() - 3
            ));
        }
    }

    output
}

/// Format results as plain text
fn format_text(results: &[ScrapedData]) -> String {
    let mut output = String::new();

    for (i, data) in results.iter().enumerate() {
        if i > 0 {
            output.push_str("\n\n");
            output.push_str(&"=".repeat(80));
            output.push_str("\n\n");
        }

        // Basic info
        output.push_str(&format!("URL: {}\n", data.url));
        output.push_str(&format!("Status: {}\n", data.status_code));

        if let Some(depth) = data.depth {
            output.push_str(&format!("Depth: {}\n", depth));
        }

        if let Some(title) = &data.title {
            output.push_str(&format!("Title: {}\n", title));
        }

        // Headings
        format_text_list(
            &mut output,
            "Headings",
            &data.headings,
            data.headings.len(), // Show all headings
            |heading| format!("  - {}\n", heading),
        );

        // Paragraphs with truncation
        if !data.paragraphs.is_empty() {
            output.push_str(&format!("\nParagraphs ({}):\n", data.paragraphs.len()));
            for (i, para) in data.paragraphs.iter().take(5).enumerate() {
                output.push_str(&format!("  {}. {}\n", i + 1, truncate_text(para, 100)));
            }
            if data.paragraphs.len() > 5 {
                output.push_str(&format!("  ... and {} more\n", data.paragraphs.len() - 5));
            }
        }

        // Links
        if !data.links.is_empty() {
            output.push_str(&format!("\nLinks ({}):\n", data.links.len()));
            for link in data.links.iter().take(10) {
                output.push_str(&format!("  - {} ({})\n", link.text, link.url));
            }
            if data.links.len() > 10 {
                output.push_str(&format!("  ... and {} more\n", data.links.len() - 10));
            }
        }

        // Images
        if !data.images.is_empty() {
            output.push_str(&format!("\nImages ({}):\n", data.images.len()));
            for img in data.images.iter().take(5) {
                output.push_str(&format!(
                    "  - {} ({})\n",
                    if img.alt.is_empty() {
                        "No alt text"
                    } else {
                        &img.alt
                    },
                    img.src
                ));
            }
            if data.images.len() > 5 {
                output.push_str(&format!("  ... and {} more\n", data.images.len() - 5));
            }
        }

        // Tables
        if !data.tables.is_empty() {
            output.push_str(&format!("\nTables ({}):\n", data.tables.len()));
            for (i, table) in data.tables.iter().take(3).enumerate() {
                output.push_str(&format!("  Table {}:\n", i + 1));
                if !table.headers.is_empty() {
                    output.push_str(&format!("    Headers: {}\n", table.headers.join(", ")));
                }
                output.push_str(&format!("    Rows: {}\n", table.rows.len()));
            }
            if data.tables.len() > 3 {
                output.push_str(&format!("  ... and {} more\n", data.tables.len() - 3));
            }
        }

        // Code Blocks
        if !data.code_blocks.is_empty() {
            output.push_str(&format!("\nCode Blocks ({}):\n", data.code_blocks.len()));
            for (i, code) in data.code_blocks.iter().take(3).enumerate() {
                let lang = code
                    .language
                    .as_ref()
                    .map(|l| format!(" ({})", l))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "  {}. {}{}\n",
                    i + 1,
                    truncate_text(&code.content, 60),
                    lang
                ));
            }
            if data.code_blocks.len() > 3 {
                output.push_str(&format!(
                    "  ... and {} more\n",
                    data.code_blocks.len() - 3
                ));
            }
        }

        // Metadata
        if let Some(metadata) = &data.metadata {
            output.push_str(&format_text_metadata(metadata));
        }

        // Custom selectors
        if !data.custom_selectors.is_empty() {
            output.push_str(&format_text_custom_selectors(&data.custom_selectors));
        }
    }

    output
}

// ========== Tests ==========

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a base URL for testing
    fn test_base_url() -> Url {
        Url::parse("https://example.com/path/page.html").unwrap()
    }

    fn test_base_url_simple() -> Url {
        Url::parse("https://example.com").unwrap()
    }

    // ========== URL Normalization Tests ==========

    #[test]
    fn test_normalize_url_absolute_https() {
        let base = test_base_url();
        let result = normalize_url(&base, "https://other.com/page");
        assert_eq!(result, Some("https://other.com/page".to_string()));
    }

    #[test]
    fn test_normalize_url_absolute_http() {
        let base = test_base_url();
        let result = normalize_url(&base, "http://other.com/page");
        assert_eq!(result, Some("http://other.com/page".to_string()));
    }

    #[test]
    fn test_normalize_url_protocol_relative() {
        let base = test_base_url();
        let result = normalize_url(&base, "//cdn.example.com/image.jpg");
        assert_eq!(result, Some("https://cdn.example.com/image.jpg".to_string()));
    }

    #[test]
    fn test_normalize_url_relative_path() {
        let base = test_base_url();
        let result = normalize_url(&base, "other-page.html");
        assert_eq!(result, Some("https://example.com/path/other-page.html".to_string()));
    }

    #[test]
    fn test_normalize_url_absolute_path() {
        let base = test_base_url();
        let result = normalize_url(&base, "/images/photo.jpg");
        assert_eq!(result, Some("https://example.com/images/photo.jpg".to_string()));
    }

    #[test]
    fn test_normalize_url_parent_directory() {
        let base = test_base_url();
        let result = normalize_url(&base, "../other/page.html");
        assert_eq!(result, Some("https://example.com/other/page.html".to_string()));
    }

    #[test]
    fn test_normalize_url_with_fragment() {
        let base = test_base_url();
        let result = normalize_url(&base, "/page#section");
        assert_eq!(result, Some("https://example.com/page#section".to_string()));
    }

    #[test]
    fn test_normalize_url_with_query_params() {
        let base = test_base_url();
        let result = normalize_url(&base, "/search?q=test&lang=en");
        assert_eq!(result, Some("https://example.com/search?q=test&lang=en".to_string()));
    }

    // ========== Domain Checking Tests ==========

    #[test]
    fn test_is_same_domain_exact_match() {
        assert!(is_same_domain("https://example.com/page", "example.com"));
    }

    #[test]
    fn test_is_same_domain_with_subdomain() {
        assert!(!is_same_domain("https://blog.example.com/page", "example.com"));
    }

    #[test]
    fn test_is_same_domain_different_domain() {
        assert!(!is_same_domain("https://other.com/page", "example.com"));
    }

    #[test]
    fn test_is_same_domain_with_path() {
        assert!(is_same_domain("https://example.com/path/to/page", "example.com"));
    }

    #[test]
    fn test_is_same_domain_invalid_url() {
        assert!(!is_same_domain("not-a-url", "example.com"));
    }

    #[test]
    fn test_is_same_domain_http_vs_https() {
        assert!(is_same_domain("http://example.com/page", "example.com"));
    }

    // ========== Title Extraction Tests ==========

    #[test]
    fn test_extract_title_present() {
        let html = r#"<html><head><title>Test Page Title</title></head><body></body></html>"#;
        let document = Html::parse_document(html);
        let title = extract_title(&document);
        assert_eq!(title, Some("Test Page Title".to_string()));
    }

    #[test]
    fn test_extract_title_with_whitespace() {
        let html = r#"<html><head><title>  Trimmed Title  </title></head><body></body></html>"#;
        let document = Html::parse_document(html);
        let title = extract_title(&document);
        assert_eq!(title, Some("Trimmed Title".to_string()));
    }

    #[test]
    fn test_extract_title_missing() {
        let html = r#"<html><head></head><body></body></html>"#;
        let document = Html::parse_document(html);
        let title = extract_title(&document);
        assert_eq!(title, None);
    }

    #[test]
    fn test_extract_title_empty() {
        let html = r#"<html><head><title></title></head><body></body></html>"#;
        let document = Html::parse_document(html);
        let title = extract_title(&document);
        assert_eq!(title, Some("".to_string()));
    }

    // ========== Headings Extraction Tests ==========

    #[test]
    fn test_extract_headings_all_levels() {
        let html = r#"
            <html><body>
                <h1>Heading 1</h1>
                <h2>Heading 2</h2>
                <h3>Heading 3</h3>
                <h4>Heading 4</h4>
                <h5>Heading 5</h5>
                <h6>Heading 6</h6>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let headings = extract_headings(&document);
        assert_eq!(headings.len(), 6);
        assert_eq!(headings[0], "Heading 1");
        assert_eq!(headings[5], "Heading 6");
    }

    #[test]
    fn test_extract_headings_empty() {
        let html = r#"<html><body><p>No headings here</p></body></html>"#;
        let document = Html::parse_document(html);
        let headings = extract_headings(&document);
        assert_eq!(headings.len(), 0);
    }

    #[test]
    fn test_extract_headings_filters_empty() {
        let html = r#"
            <html><body>
                <h1>Valid Heading</h1>
                <h2>   </h2>
                <h3></h3>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let headings = extract_headings(&document);
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0], "Valid Heading");
    }

    #[test]
    fn test_extract_headings_trims_whitespace() {
        let html = r#"<html><body><h1>  Trimmed  </h1></body></html>"#;
        let document = Html::parse_document(html);
        let headings = extract_headings(&document);
        assert_eq!(headings[0], "Trimmed");
    }

    // ========== Paragraphs Extraction Tests ==========

    #[test]
    fn test_extract_paragraphs_multiple() {
        let html = r#"
            <html><body>
                <p>First paragraph</p>
                <p>Second paragraph</p>
                <p>Third paragraph</p>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let paragraphs = extract_paragraphs(&document);
        assert_eq!(paragraphs.len(), 3);
        assert_eq!(paragraphs[0], "First paragraph");
    }

    #[test]
    fn test_extract_paragraphs_filters_empty() {
        let html = r#"
            <html><body>
                <p>Valid paragraph</p>
                <p></p>
                <p>   </p>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let paragraphs = extract_paragraphs(&document);
        assert_eq!(paragraphs.len(), 1);
        assert_eq!(paragraphs[0], "Valid paragraph");
    }

    #[test]
    fn test_extract_paragraphs_none() {
        let html = r#"<html><body><div>Not a paragraph</div></body></html>"#;
        let document = Html::parse_document(html);
        let paragraphs = extract_paragraphs(&document);
        assert_eq!(paragraphs.len(), 0);
    }

    // ========== Links Extraction Tests ==========

    #[test]
    fn test_extract_links_absolute() {
        let html = r#"
            <html><body>
                <a href="https://example.com/page">Link Text</a>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let links = extract_links(&document, &base_url);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Link Text");
        assert_eq!(links[0].url, "https://example.com/page");
    }

    #[test]
    fn test_extract_links_relative() {
        let html = r#"
            <html><body>
                <a href="/about">About</a>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let links = extract_links(&document, &base_url);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "About");
        assert_eq!(links[0].url, "https://example.com/about");
    }

    #[test]
    fn test_extract_links_empty_text_uses_href() {
        let html = r#"
            <html><body>
                <a href="/contact"></a>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let links = extract_links(&document, &base_url);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "/contact");
    }

    #[test]
    fn test_extract_links_no_href() {
        let html = r#"
            <html><body>
                <a>No href attribute</a>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let links = extract_links(&document, &base_url);

        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_extract_links_protocol_relative() {
        let html = r#"
            <html><body>
                <a href="//cdn.example.com/page">CDN Link</a>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let links = extract_links(&document, &base_url);

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://cdn.example.com/page");
    }

    // ========== Images Extraction Tests ==========

    #[test]
    fn test_extract_images_absolute() {
        let html = r#"
            <html><body>
                <img src="https://example.com/image.jpg" alt="Test Image">
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let images = extract_images(&document, &base_url);

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].alt, "Test Image");
        assert_eq!(images[0].src, "https://example.com/image.jpg");
    }

    #[test]
    fn test_extract_images_relative() {
        let html = r#"
            <html><body>
                <img src="/images/photo.jpg" alt="Photo">
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let images = extract_images(&document, &base_url);

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://example.com/images/photo.jpg");
    }

    #[test]
    fn test_extract_images_no_alt() {
        let html = r#"
            <html><body>
                <img src="https://example.com/image.jpg">
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let images = extract_images(&document, &base_url);

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].alt, "");
    }

    #[test]
    fn test_extract_images_protocol_relative() {
        let html = r#"
            <html><body>
                <img src="//cdn.example.com/image.jpg" alt="CDN Image">
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let images = extract_images(&document, &base_url);

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].src, "https://cdn.example.com/image.jpg");
    }

    #[test]
    fn test_extract_images_no_src() {
        let html = r#"
            <html><body>
                <img alt="No source">
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let base_url = test_base_url_simple();
        let images = extract_images(&document, &base_url);

        assert_eq!(images.len(), 0);
    }

    // ========== Metadata Extraction Tests ==========

    #[test]
    fn test_extract_metadata_complete() {
        let html = r#"
            <html><head>
                <meta name="description" content="Test description">
                <meta name="keywords" content="test, keywords">
                <meta name="author" content="Test Author">
                <meta property="og:title" content="OG Title">
                <meta property="og:description" content="OG Description">
                <meta property="og:image" content="https://example.com/og.jpg">
                <meta property="og:url" content="https://example.com">
                <link rel="canonical" href="https://example.com/canonical">
                <link rel="icon" href="/favicon.ico">
            </head><body></body></html>
        "#;
        let document = Html::parse_document(html);
        let metadata = extract_metadata(&document);

        assert_eq!(metadata.description, Some("Test description".to_string()));
        assert_eq!(metadata.keywords, Some("test, keywords".to_string()));
        assert_eq!(metadata.author, Some("Test Author".to_string()));
        assert_eq!(metadata.og_title, Some("OG Title".to_string()));
        assert_eq!(metadata.og_description, Some("OG Description".to_string()));
        assert_eq!(metadata.og_image, Some("https://example.com/og.jpg".to_string()));
        assert_eq!(metadata.og_url, Some("https://example.com".to_string()));
        assert_eq!(metadata.canonical_url, Some("https://example.com/canonical".to_string()));
        assert_eq!(metadata.favicon, Some("/favicon.ico".to_string()));
    }

    #[test]
    fn test_extract_metadata_empty() {
        let html = r#"<html><head></head><body></body></html>"#;
        let document = Html::parse_document(html);
        let metadata = extract_metadata(&document);

        assert_eq!(metadata.description, None);
        assert_eq!(metadata.keywords, None);
        assert_eq!(metadata.author, None);
        assert_eq!(metadata.og_title, None);
    }

    #[test]
    fn test_extract_metadata_partial() {
        let html = r#"
            <html><head>
                <meta name="description" content="Just description">
                <meta property="og:title" content="Just OG title">
            </head><body></body></html>
        "#;
        let document = Html::parse_document(html);
        let metadata = extract_metadata(&document);

        assert_eq!(metadata.description, Some("Just description".to_string()));
        assert_eq!(metadata.og_title, Some("Just OG title".to_string()));
        assert_eq!(metadata.keywords, None);
        assert_eq!(metadata.author, None);
    }

    #[test]
    fn test_extract_metadata_shortcut_icon() {
        let html = r#"
            <html><head>
                <link rel="shortcut icon" href="/favicon.png">
            </head><body></body></html>
        "#;
        let document = Html::parse_document(html);
        let metadata = extract_metadata(&document);

        assert_eq!(metadata.favicon, Some("/favicon.png".to_string()));
    }

    // ========== Custom Selectors Tests ==========

    #[test]
    fn test_process_custom_selectors_valid() {
        let html = r#"
            <html><body>
                <div class="item">Item 1</div>
                <div class="item">Item 2</div>
                <div class="item">Item 3</div>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let selectors = vec![".item".to_string()];
        let results = process_custom_selectors(&document, &selectors).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].selector, ".item");
        assert_eq!(results[0].matches.len(), 3);
        assert_eq!(results[0].matches[0], "Item 1");
    }

    #[test]
    fn test_process_custom_selectors_multiple() {
        let html = r#"
            <html><body>
                <h1>Heading</h1>
                <p class="intro">Intro paragraph</p>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let selectors = vec!["h1".to_string(), ".intro".to_string()];
        let results = process_custom_selectors(&document, &selectors).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].matches[0], "Heading");
        assert_eq!(results[1].matches[0], "Intro paragraph");
    }

    #[test]
    fn test_process_custom_selectors_no_matches() {
        let html = r#"<html><body><p>Content</p></body></html>"#;
        let document = Html::parse_document(html);
        let selectors = vec![".nonexistent".to_string()];
        let results = process_custom_selectors(&document, &selectors).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matches.len(), 0);
    }

    #[test]
    fn test_process_custom_selectors_invalid() {
        let html = r#"<html><body></body></html>"#;
        let document = Html::parse_document(html);
        let selectors = vec!["invalid[[[selector".to_string()];
        let result = process_custom_selectors(&document, &selectors);

        assert!(result.is_err());
    }

    #[test]
    fn test_process_custom_selectors_filters_empty() {
        let html = r#"
            <html><body>
                <div class="item">Valid</div>
                <div class="item">   </div>
                <div class="item"></div>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let selectors = vec![".item".to_string()];
        let results = process_custom_selectors(&document, &selectors).unwrap();

        assert_eq!(results[0].matches.len(), 1);
        assert_eq!(results[0].matches[0], "Valid");
    }

    // ========== Crawl Queue Tests ==========

    #[test]
    fn test_should_add_to_crawl_queue_same_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "https://example.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, Some("https://example.com/page".to_string()));
    }

    #[test]
    fn test_should_add_to_crawl_queue_different_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "https://other.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_should_add_to_crawl_queue_already_visited() {
        let base_url = Url::parse("https://example.com").unwrap();
        let mut visited = HashSet::new();
        visited.insert("https://example.com/page".to_string());
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "https://example.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_should_add_to_crawl_queue_relative_url() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "/about",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, Some("https://example.com/about".to_string()));
    }

    #[test]
    fn test_should_add_to_crawl_queue_relative_different_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        // This should resolve to example.com domain
        let result = should_add_to_crawl_queue(
            "../page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert!(result.is_some());
        assert!(result.unwrap().starts_with("https://example.com"));
    }

    // ========== Domain Filtering Tests ==========

    #[test]
    fn test_domain_filtering_allow_list_includes_allowed_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let mut allow_domains = HashSet::new();
        allow_domains.insert("docs.example.com".to_string());
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "https://docs.example.com/api",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, Some("https://docs.example.com/api".to_string()));
    }

    #[test]
    fn test_domain_filtering_allow_list_blocks_non_allowed_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let mut allow_domains = HashSet::new();
        allow_domains.insert("docs.example.com".to_string());
        let block_domains = HashSet::new();

        // other.com is not in allow list, should be blocked
        let result = should_add_to_crawl_queue(
            "https://other.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_domain_filtering_allow_list_always_includes_base_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let mut allow_domains = HashSet::new();
        allow_domains.insert("docs.example.com".to_string());
        let block_domains = HashSet::new();

        // Base domain should always be allowed even if not explicitly in allow list
        let result = should_add_to_crawl_queue(
            "https://example.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, Some("https://example.com/page".to_string()));
    }

    #[test]
    fn test_domain_filtering_block_list_blocks_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let mut block_domains = HashSet::new();
        block_domains.insert("ads.example.com".to_string());

        let result = should_add_to_crawl_queue(
            "https://ads.example.com/tracker",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_domain_filtering_block_list_allows_non_blocked_same_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let mut block_domains = HashSet::new();
        block_domains.insert("ads.example.com".to_string());

        // Base domain should still work
        let result = should_add_to_crawl_queue(
            "https://example.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, Some("https://example.com/page".to_string()));
    }

    #[test]
    fn test_domain_filtering_cross_domain_allows_any_domain() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let block_domains = HashSet::new();

        let result = should_add_to_crawl_queue(
            "https://completely-different.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            true, // cross_domain enabled
        );

        assert_eq!(
            result,
            Some("https://completely-different.com/page".to_string())
        );
    }

    #[test]
    fn test_domain_filtering_cross_domain_respects_block_list() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let allow_domains = HashSet::new();
        let mut block_domains = HashSet::new();
        block_domains.insert("blocked.com".to_string());

        // Even with cross-domain enabled, blocked domains should still be blocked
        let result = should_add_to_crawl_queue(
            "https://blocked.com/page",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            true, // cross_domain enabled
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_domain_filtering_mixed_allow_and_block() {
        let base_url = Url::parse("https://example.com").unwrap();
        let visited = HashSet::new();
        let mut allow_domains = HashSet::new();
        allow_domains.insert("docs.example.com".to_string());
        allow_domains.insert("api.example.com".to_string());
        let mut block_domains = HashSet::new();
        block_domains.insert("api.example.com".to_string());

        // Block list takes precedence over allow list
        let result = should_add_to_crawl_queue(
            "https://api.example.com/endpoint",
            &base_url,
            "example.com",
            &visited,
            &allow_domains,
            &block_domains,
            false,
        );

        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_domain_list_comma_separated() {
        let domains = parse_domain_list("example.com,docs.example.com,api.example.com");
        assert_eq!(domains.len(), 3);
        assert!(domains.contains("example.com"));
        assert!(domains.contains("docs.example.com"));
        assert!(domains.contains("api.example.com"));
    }

    #[test]
    fn test_parse_domain_list_with_whitespace() {
        let domains = parse_domain_list("  example.com  , docs.example.com , api.example.com  ");
        assert_eq!(domains.len(), 3);
        assert!(domains.contains("example.com"));
        assert!(domains.contains("docs.example.com"));
        assert!(domains.contains("api.example.com"));
    }

    #[test]
    fn test_parse_domain_list_empty_entries() {
        let domains = parse_domain_list("example.com,,docs.example.com,  ,api.example.com");
        assert_eq!(domains.len(), 3);
        assert!(domains.contains("example.com"));
        assert!(domains.contains("docs.example.com"));
        assert!(domains.contains("api.example.com"));
    }

    #[test]
    fn test_parse_domain_list_case_insensitive() {
        let domains = parse_domain_list("Example.COM,DOCS.example.com,api.EXAMPLE.com");
        assert_eq!(domains.len(), 3);
        // All should be lowercased
        assert!(domains.contains("example.com"));
        assert!(domains.contains("docs.example.com"));
        assert!(domains.contains("api.example.com"));
    }

    // ========== Text Formatting Helper Tests ==========

    #[test]
    fn test_truncate_text_short() {
        let text = "Short text";
        let result = truncate_text(text, 100);
        assert_eq!(result, "Short text");
    }

    #[test]
    fn test_truncate_text_long() {
        let text = "This is a very long piece of text that should be truncated at the specified length with ellipsis added";
        let result = truncate_text(text, 20);
        assert_eq!(result, "This is a very long ...");
        assert_eq!(result.len(), 23); // 20 chars + "..."
    }

    #[test]
    fn test_truncate_text_exact_length() {
        let text = "12345678901234567890"; // exactly 20 chars
        let result = truncate_text(text, 20);
        assert_eq!(result, "12345678901234567890");
    }

    #[test]
    fn test_format_text_metadata() {
        let metadata = Metadata {
            description: Some("Test description".to_string()),
            keywords: Some("test, rust".to_string()),
            author: Some("Author Name".to_string()),
            og_title: Some("OG Title".to_string()),
            og_description: None,
            og_image: Some("https://example.com/image.jpg".to_string()),
            og_url: None,
            canonical_url: None,
            favicon: None,
        };

        let result = format_text_metadata(&metadata);
        assert!(result.contains("Description: Test description"));
        assert!(result.contains("Keywords: test, rust"));
        assert!(result.contains("Author: Author Name"));
        assert!(result.contains("OG Title: OG Title"));
        assert!(result.contains("OG Image: https://example.com/image.jpg"));
    }

    #[test]
    fn test_format_text_custom_selectors() {
        let selectors = vec![
            CustomSelectorResult {
                selector: ".item".to_string(),
                matches: vec!["Match 1".to_string(), "Match 2".to_string()],
            },
        ];

        let result = format_text_custom_selectors(&selectors);
        assert!(result.contains("'.item' (2 matches)"));
        assert!(result.contains("1. Match 1"));
        assert!(result.contains("2. Match 2"));
    }

    #[test]
    fn test_format_text_custom_selectors_truncated() {
        let selectors = vec![
            CustomSelectorResult {
                selector: ".item".to_string(),
                matches: vec![
                    "Match 1".to_string(),
                    "Match 2".to_string(),
                    "Match 3".to_string(),
                    "Match 4".to_string(),
                ],
            },
        ];

        let result = format_text_custom_selectors(&selectors);
        assert!(result.contains("... and 1 more"));
    }

    // ========== Tables Extraction Tests ==========

    #[test]
    fn test_extract_tables_with_headers() {
        let html = r#"
            <html><body>
                <table>
                    <tr><th>Name</th><th>Age</th></tr>
                    <tr><td>Alice</td><td>30</td></tr>
                    <tr><td>Bob</td><td>25</td></tr>
                </table>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let tables = extract_tables(&document);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Name", "Age"]);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0], vec!["Alice", "30"]);
        assert_eq!(tables[0].rows[1], vec!["Bob", "25"]);
    }

    #[test]
    fn test_extract_tables_without_headers() {
        let html = r#"
            <html><body>
                <table>
                    <tr><td>Data 1</td><td>Data 2</td></tr>
                    <tr><td>Data 3</td><td>Data 4</td></tr>
                </table>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let tables = extract_tables(&document);

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers.len(), 0);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0], vec!["Data 1", "Data 2"]);
    }

    #[test]
    fn test_extract_tables_multiple() {
        let html = r#"
            <html><body>
                <table>
                    <tr><th>Column 1</th></tr>
                    <tr><td>Value 1</td></tr>
                </table>
                <table>
                    <tr><td>Table 2</td></tr>
                </table>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let tables = extract_tables(&document);

        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].headers, vec!["Column 1"]);
        assert_eq!(tables[1].rows.len(), 1);
    }

    #[test]
    fn test_extract_tables_none() {
        let html = r#"<html><body><p>No tables here</p></body></html>"#;
        let document = Html::parse_document(html);
        let tables = extract_tables(&document);

        assert_eq!(tables.len(), 0);
    }

    #[test]
    fn test_extract_tables_empty() {
        let html = r#"<html><body><table></table></body></html>"#;
        let document = Html::parse_document(html);
        let tables = extract_tables(&document);

        assert_eq!(tables.len(), 0);
    }

    // ========== Code Blocks Extraction Tests ==========

    #[test]
    fn test_extract_code_blocks_pre_code() {
        let html = r#"
            <html><body>
                <pre><code>function hello() {
    console.log("Hello");
}</code></pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert!(code_blocks[0].content.contains("function hello()"));
        assert_eq!(code_blocks[0].language, None);
    }

    #[test]
    fn test_extract_code_blocks_with_language() {
        let html = r#"
            <html><body>
                <pre><code class="language-rust">fn main() {
    println!("Hello");
}</code></pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert!(code_blocks[0].content.contains("fn main()"));
        assert_eq!(code_blocks[0].language, Some("rust".to_string()));
    }

    #[test]
    fn test_extract_code_blocks_lang_prefix() {
        let html = r#"
            <html><body>
                <pre><code class="lang-python">def hello():
    print("Hello")</code></pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert!(code_blocks[0].content.contains("def hello()"));
        assert_eq!(code_blocks[0].language, Some("python".to_string()));
    }

    #[test]
    fn test_extract_code_blocks_pre_only() {
        let html = r#"
            <html><body>
                <pre>Plain preformatted text</pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert_eq!(code_blocks[0].content, "Plain preformatted text");
        assert_eq!(code_blocks[0].language, None);
    }

    #[test]
    fn test_extract_code_blocks_inline_code() {
        let html = r#"
            <html><body>
                <p>Use the <code>print()</code> function</p>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert_eq!(code_blocks[0].content, "print()");
        assert_eq!(code_blocks[0].language, None);
    }

    #[test]
    fn test_extract_code_blocks_multiple() {
        let html = r#"
            <html><body>
                <pre><code>code block 1</code></pre>
                <pre><code>code block 2</code></pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 2);
        assert_eq!(code_blocks[0].content, "code block 1");
        assert_eq!(code_blocks[1].content, "code block 2");
    }

    #[test]
    fn test_extract_code_blocks_none() {
        let html = r#"<html><body><p>No code blocks here</p></body></html>"#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 0);
    }

    #[test]
    fn test_extract_code_blocks_filters_empty() {
        let html = r#"
            <html><body>
                <pre><code>Valid code</code></pre>
                <pre><code>   </code></pre>
                <pre><code></code></pre>
            </body></html>
        "#;
        let document = Html::parse_document(html);
        let code_blocks = extract_code_blocks(&document);

        assert_eq!(code_blocks.len(), 1);
        assert_eq!(code_blocks[0].content, "Valid code");
    }

    // ========== JSON Format Tests ==========

    #[test]
    fn test_format_json_single_result() {
        let data = vec![ScrapedData {
            url: "https://example.com".to_string(),
            status_code: 200,
            title: Some("Test".to_string()),
            headings: vec!["H1".to_string()],
            paragraphs: vec!["Para".to_string()],
            links: vec![],
            images: vec![],
            tables: vec![],
            code_blocks: vec![],
            metadata: None,
            custom_selectors: vec![],
            depth: None,
        }];

        let result = format_json(&data).unwrap();
        assert!(result.contains("https://example.com"));
        assert!(result.contains("Test"));
        assert!(result.contains("H1"));
    }

    #[test]
    fn test_format_json_multiple_results() {
        let data = vec![
            ScrapedData {
                url: "https://example.com/1".to_string(),
                status_code: 200,
                title: Some("Page 1".to_string()),
                headings: vec![],
                paragraphs: vec![],
                links: vec![],
                images: vec![],
                tables: vec![],
                code_blocks: vec![],
                metadata: None,
                custom_selectors: vec![],
                depth: None,
            },
            ScrapedData {
                url: "https://example.com/2".to_string(),
                status_code: 200,
                title: Some("Page 2".to_string()),
                headings: vec![],
                paragraphs: vec![],
                links: vec![],
                images: vec![],
                tables: vec![],
                code_blocks: vec![],
                metadata: None,
                custom_selectors: vec![],
                depth: None,
            },
        ];

        let result = format_json(&data).unwrap();
        assert!(result.contains("Page 1"));
        assert!(result.contains("Page 2"));
    }

    // ========== CSV Format Tests ==========

    #[test]
    fn test_format_csv_headers() {
        let data = vec![ScrapedData {
            url: "https://example.com".to_string(),
            status_code: 200,
            title: Some("Test".to_string()),
            headings: vec![],
            paragraphs: vec![],
            links: vec![],
            images: vec![],
            tables: vec![],
            code_blocks: vec![],
            metadata: None,
            custom_selectors: vec![],
            depth: None,
        }];

        let result = format_csv(&data).unwrap();
        let lines: Vec<&str> = result.lines().collect();

        assert_eq!(lines[0], "url,status_code,title,headings_count,paragraphs_count,links_count,images_count,tables_count,code_blocks_count,depth");
    }

    #[test]
    fn test_format_csv_data_row() {
        let data = vec![ScrapedData {
            url: "https://example.com".to_string(),
            status_code: 200,
            title: Some("Test".to_string()),
            headings: vec!["H1".to_string()],
            paragraphs: vec!["P1".to_string(), "P2".to_string()],
            links: vec![],
            images: vec![],
            tables: vec![],
            code_blocks: vec![],
            metadata: None,
            custom_selectors: vec![],
            depth: Some(1),
        }];

        let result = format_csv(&data).unwrap();
        let lines: Vec<&str> = result.lines().collect();

        assert_eq!(lines[1], "https://example.com,200,Test,1,2,0,0,0,0,1");
    }

    // ========== Error Handling Tests ==========

    #[test]
    fn test_classify_http_status_success() {
        let result = classify_http_status(200, "https://example.com");
        assert!(result.is_ok());
    }

    #[test]
    fn test_classify_http_status_404() {
        let result = classify_http_status(404, "https://example.com/missing");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Not Found"));
        assert!(err.to_string().contains("/missing"));
    }

    #[test]
    fn test_classify_http_status_403() {
        let result = classify_http_status(403, "https://example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Forbidden"));
        assert!(err.to_string().contains("bot protection"));
    }

    #[test]
    fn test_classify_http_status_429() {
        let result = classify_http_status(429, "https://example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Rate limited"));
        assert!(err.to_string().contains("Too many requests"));
    }

    #[test]
    fn test_classify_http_status_500() {
        let result = classify_http_status(500, "https://example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Internal Server Error"));
    }

    #[test]
    fn test_classify_http_status_503() {
        let result = classify_http_status(503, "https://example.com");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Service Unavailable"));
    }

    #[test]
    fn test_detect_anti_bot_cloudflare() {
        let html = r#"<html><body><div class="cf-browser-verification">Checking your browser</div></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Cloudflare"));
    }

    #[test]
    fn test_detect_anti_bot_cloudflare_challenge() {
        let html = r#"<html><body><div>Just a moment...</div><script>challenge-platform</script></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("JavaScript challenge"));
    }

    #[test]
    fn test_detect_anti_bot_recaptcha() {
        let html = r#"<html><body><div class="g-recaptcha"></div></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("reCAPTCHA"));
    }

    #[test]
    fn test_detect_anti_bot_hcaptcha() {
        let html = r#"<html><body><div class="h-captcha"></div></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("hCaptcha"));
    }

    #[test]
    fn test_detect_anti_bot_perimeterx() {
        let html = r#"<html><body><div class="px-captcha"></div></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("PerimeterX"));
    }

    #[test]
    fn test_detect_anti_bot_datadome() {
        let html = r#"<html><body><script src="datadome.js"></script></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("DataDome"));
    }

    #[test]
    fn test_detect_anti_bot_title_access_denied() {
        let html = r#"<html><body>Content</body></html>"#;
        let result = detect_anti_bot_features(html, Some("Access Denied - Forbidden"));
        assert!(result.is_some());
        assert!(result.unwrap().contains("Access restriction detected"));
    }

    #[test]
    fn test_detect_anti_bot_title_blocked() {
        let html = r#"<html><body>Content</body></html>"#;
        let result = detect_anti_bot_features(html, Some("You have been blocked"));
        assert!(result.is_some());
        assert!(result.unwrap().contains("Access restriction"));
    }

    #[test]
    fn test_detect_anti_bot_none() {
        let html = r#"<html><body><p>Normal content here</p></body></html>"#;
        let result = detect_anti_bot_features(html, Some("Normal Page"));
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_anti_bot_cloudflare_ray_id() {
        let html = r#"<html><body><div>Cloudflare Ray ID: abc123</div></body></html>"#;
        let result = detect_anti_bot_features(html, None);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Cloudflare error page"));
    }

    // ========== URL File Reading Tests ==========

    #[test]
    fn test_read_urls_from_file_valid() {
        use std::io::Write;

        // Create a temporary file with valid URLs
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_valid.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "https://example.com").unwrap();
        writeln!(file, "https://google.com").unwrap();
        writeln!(file, "https://rust-lang.org").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_ok());
        let urls = result.unwrap();
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "https://google.com");
        assert_eq!(urls[2], "https://rust-lang.org");

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }

    #[test]
    fn test_read_urls_from_file_with_comments_and_empty_lines() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_comments.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "# This is a comment").unwrap();
        writeln!(file, "https://example.com").unwrap();
        writeln!(file, "").unwrap();
        writeln!(file, "  ").unwrap();
        writeln!(file, "# Another comment").unwrap();
        writeln!(file, "https://google.com").unwrap();
        writeln!(file, "").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_ok());
        let urls = result.unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "https://google.com");

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }

    #[test]
    fn test_read_urls_from_file_mixed_valid_invalid() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_mixed.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "https://example.com").unwrap();
        writeln!(file, "not-a-valid-url").unwrap();
        writeln!(file, "https://google.com").unwrap();
        writeln!(file, "also invalid").unwrap();
        writeln!(file, "https://rust-lang.org").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_ok());
        let urls = result.unwrap();
        // Should only get valid URLs, invalid ones are logged as warnings
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "https://google.com");
        assert_eq!(urls[2], "https://rust-lang.org");

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }

    #[test]
    fn test_read_urls_from_file_not_found() {
        let result = read_urls_from_file("/nonexistent/path/to/urls.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to open URL file"));
    }

    #[test]
    fn test_read_urls_from_file_no_valid_urls() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_empty.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "# Only comments").unwrap();
        writeln!(file, "").unwrap();
        writeln!(file, "# More comments").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No valid URLs found"));

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }

    #[test]
    fn test_read_urls_from_file_only_invalid_urls() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_invalid.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "not-a-url").unwrap();
        writeln!(file, "also-not-a-url").unwrap();
        writeln!(file, "still-not-valid").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No valid URLs found"));

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }

    #[test]
    fn test_read_urls_from_file_whitespace_handling() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_urls_whitespace.txt");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "  https://example.com  ").unwrap();
        writeln!(file, "\thttps://google.com\t").unwrap();
        writeln!(file, " \t https://rust-lang.org \t ").unwrap();
        drop(file);

        let result = read_urls_from_file(file_path.to_str().unwrap());
        assert!(result.is_ok());
        let urls = result.unwrap();
        assert_eq!(urls.len(), 3);
        // URLs should be trimmed
        assert_eq!(urls[0], "https://example.com");
        assert_eq!(urls[1], "https://google.com");
        assert_eq!(urls[2], "https://rust-lang.org");

        // Cleanup
        std::fs::remove_file(&file_path).ok();
    }
}
