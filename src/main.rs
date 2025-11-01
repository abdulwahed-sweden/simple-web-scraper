use anyhow::Result;
use clap::Parser;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
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
}

/// CLI arguments
#[derive(Parser, Debug)]
#[command(name = "simple-web-scraper")]
#[command(about = "A simple but powerful web scraper", long_about = None)]
struct Args {
    /// URL(s) to scrape (can provide multiple)
    #[arg(required = true)]
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

    /// Extract metadata (Open Graph, meta tags)
    #[arg(long)]
    metadata: bool,

    /// Save output to file
    #[arg(short, long)]
    output: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

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

    let mut results = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((start_url.clone(), 0usize));

    log::info!("🕷️  Starting crawl from: {}", start_url);
    log::info!("📊 Max depth: {}, Max pages: {}", args.max_depth, args.max_pages);

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
                        if let Ok(link_url) = Url::parse(&link.url) {
                            // Only crawl links from the same domain
                            if link_url.domain() == Some(base_domain) {
                                let link_str = link_url.to_string();
                                if !visited.contains(&link_str) {
                                    queue.push_back((link_str, depth + 1));
                                }
                            }
                        } else if let Ok(absolute_url) = base_url.join(&link.url) {
                            // Handle relative URLs
                            if absolute_url.domain() == Some(base_domain) {
                                let link_str = absolute_url.to_string();
                                if !visited.contains(&link_str) {
                                    queue.push_back((link_str, depth + 1));
                                }
                            }
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

    let client = client_builder.build()?;

    // Fetch the page
    let response = client.get(url).send().await?;
    let status_code = response.status().as_u16();

    if !response.status().is_success() {
        log::warn!("Non-success status code: {}", status_code);
    }

    let html = response.text().await?;
    let document = Html::parse_document(&html);

    // Extract title
    let title_selector = Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string());

    // Extract headings (h1, h2, h3, h4, h5, h6)
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

    // Extract paragraphs
    let p_selector = Selector::parse("p").unwrap();
    let paragraphs: Vec<String> = document
        .select(&p_selector)
        .map(|el| el.text().collect::<String>().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    // Extract links (convert relative URLs to absolute)
    let base_url = Url::parse(url)?;
    let a_selector = Selector::parse("a").unwrap();
    let links: Vec<Link> = document
        .select(&a_selector)
        .filter_map(|el| {
            let href = el.value().attr("href")?;
            let text = el.text().collect::<String>().trim().to_string();

            // Convert to absolute URL if needed
            let absolute_url = if href.starts_with("http://") || href.starts_with("https://") {
                href.to_string()
            } else {
                base_url.join(href).ok()?.to_string()
            };

            Some(Link {
                text: if text.is_empty() {
                    href.to_string()
                } else {
                    text
                },
                url: absolute_url,
            })
        })
        .collect();

    // Extract images (convert relative URLs to absolute)
    let img_selector = Selector::parse("img").unwrap();
    let images: Vec<Image> = document
        .select(&img_selector)
        .filter_map(|el| {
            let src = el.value().attr("src")?;
            let alt = el.value().attr("alt").unwrap_or("").to_string();

            // Convert to absolute URL if needed
            let absolute_src = if src.starts_with("http://") || src.starts_with("https://") {
                src.to_string()
            } else if src.starts_with("//") {
                format!("https:{}", src)
            } else {
                base_url.join(src).ok()?.to_string()
            };

            Some(Image {
                alt,
                src: absolute_src,
            })
        })
        .collect();

    // Extract metadata if requested
    let metadata = if args.metadata {
        Some(extract_metadata(&document))
    } else {
        None
    };

    // Process custom selectors if provided
    let mut custom_selectors = Vec::new();
    for selector_str in &args.selector {
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

                custom_selectors.push(CustomSelectorResult {
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

    Ok(ScrapedData {
        url: url.to_string(),
        status_code,
        title,
        headings,
        paragraphs,
        links,
        images,
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
            &data.depth.map(|d| d.to_string()).unwrap_or_default(),
        ])?;
    }

    Ok(String::from_utf8(writer.into_inner()?)?)
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

        output.push_str(&format!("URL: {}\n", data.url));
        output.push_str(&format!("Status: {}\n", data.status_code));

        if let Some(depth) = data.depth {
            output.push_str(&format!("Depth: {}\n", depth));
        }

        if let Some(title) = &data.title {
            output.push_str(&format!("Title: {}\n", title));
        }

        if !data.headings.is_empty() {
            output.push_str(&format!("\nHeadings ({}):\n", data.headings.len()));
            for heading in &data.headings {
                output.push_str(&format!("  - {}\n", heading));
            }
        }

        if !data.paragraphs.is_empty() {
            output.push_str(&format!("\nParagraphs ({}):\n", data.paragraphs.len()));
            for (i, para) in data.paragraphs.iter().take(5).enumerate() {
                let preview = if para.len() > 100 {
                    format!("{}...", &para[..100])
                } else {
                    para.clone()
                };
                output.push_str(&format!("  {}. {}\n", i + 1, preview));
            }
            if data.paragraphs.len() > 5 {
                output.push_str(&format!("  ... and {} more\n", data.paragraphs.len() - 5));
            }
        }

        if !data.links.is_empty() {
            output.push_str(&format!("\nLinks ({}):\n", data.links.len()));
            for link in data.links.iter().take(10) {
                output.push_str(&format!("  - {} ({})\n", link.text, link.url));
            }
            if data.links.len() > 10 {
                output.push_str(&format!("  ... and {} more\n", data.links.len() - 10));
            }
        }

        if !data.images.is_empty() {
            output.push_str(&format!("\nImages ({}):\n", data.images.len()));
            for img in data.images.iter().take(5) {
                output.push_str(&format!(
                    "  - {} ({})\n",
                    if img.alt.is_empty() { "No alt text" } else { &img.alt },
                    img.src
                ));
            }
            if data.images.len() > 5 {
                output.push_str(&format!("  ... and {} more\n", data.images.len() - 5));
            }
        }

        if let Some(metadata) = &data.metadata {
            output.push_str("\nMetadata:\n");
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
        }

        if !data.custom_selectors.is_empty() {
            output.push_str("\nCustom Selectors:\n");
            for result in &data.custom_selectors {
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
        }
    }

    output
}
