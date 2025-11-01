# Simple Web Scraper

A powerful yet lightweight Rust web scraper with advanced features including crawling, metadata extraction, and multiple output formats - all in a single source file!

## Features

### Core Scraping
- Extract page titles, headings (h1-h6), paragraphs, links, and images
- Automatic URL normalization (relative to absolute)
- HTTP status code tracking
- Robust error handling with helpful messages

### Advanced Features
- **Multiple Output Formats**: JSON, CSV, or plain text
- **Metadata Extraction**: Open Graph tags, meta descriptions, keywords, author, favicon
- **Custom CSS Selectors**: Extract any content using CSS selectors
- **Web Crawling**: Follow links with configurable depth and page limits
- **Concurrent Scraping**: Scrape multiple URLs in one command
- **Rate Limiting**: Configurable delays to be polite to servers
- **Proxy Support**: Route requests through HTTP/HTTPS proxies
- **Custom User-Agent**: Set your own user-agent string
- **Timeout Control**: Prevent hanging requests
- **Verbose/Quiet Modes**: Control logging output

## Installation

```bash
cargo build --release
```

The binary will be available at `./target/release/simple-web-scraper`

## Quick Start

### Basic Usage

```bash
# Scrape a single URL (JSON output)
cargo run --release -- https://books.toscrape.com

# Scrape multiple URLs
cargo run --release -- https://books.toscrape.com https://quotes.toscrape.com

# Save to file
cargo run --release -- https://books.toscrape.com -o output.json
```

### Output Formats

```bash
# JSON (default)
cargo run --release -- https://books.toscrape.com

# CSV format
cargo run --release -- https://books.toscrape.com --format csv

# Plain text
cargo run --release -- https://books.toscrape.com --format text
```

### Metadata Extraction

```bash
# Extract Open Graph tags and meta information
cargo run --release -- https://books.toscrape.com --metadata --format text
```

### Custom Selectors

```bash
# Extract book titles and prices
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color"

# Extract multiple data points from books
cargo run --release -- https://books.toscrape.com \
  -s "h3 a" \
  -s ".star-rating" \
  -s ".price_color"
```

### Web Crawling

```bash
# Crawl the books website (follows internal links)
cargo run --release -- https://books.toscrape.com --crawl

# Control crawl depth and max pages
cargo run --release -- https://books.toscrape.com --crawl --max-depth 3 --max-pages 50

# Crawl with metadata extraction
cargo run --release -- https://books.toscrape.com --crawl --metadata --max-pages 20
```

### Advanced Configuration

```bash
# Use custom user-agent
cargo run --release -- https://books.toscrape.com -u "MyBot/1.0"

# Use proxy
cargo run --release -- https://books.toscrape.com -p http://proxy.example.com:8080

# Set custom timeout (in seconds)
cargo run --release -- https://books.toscrape.com -t 60

# Control rate limiting (delay in milliseconds)
cargo run --release -- https://books.toscrape.com \
  https://quotes.toscrape.com -d 2000

# Verbose logging
cargo run --release -- https://books.toscrape.com -v

# Quiet mode (no logs, just output)
cargo run --release -- https://books.toscrape.com -q
```

### Real-World Examples

```bash
# Scrape book information with prices
cargo run --release -- https://books.toscrape.com \
  -s "h3 a" \
  -s ".price_color" \
  -s ".star-rating" \
  --format csv -o books.csv

# Crawl all book categories and extract metadata
cargo run --release -- https://books.toscrape.com \
  --crawl --max-depth 2 --max-pages 30 \
  --metadata --delay 1000 \
  -o books-catalog.json

# Extract specific book data with custom selectors
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3" \
  -s "article.product_pod .price_color" \
  -s "article.product_pod .instock" \
  --format text
```

## Command-Line Options

```
Usage: simple-web-scraper [OPTIONS] <URLS>...

Arguments:
  <URLS>...  URL(s) to scrape (can provide multiple)

Options:
  -f, --format <FORMAT>          Output format: json, csv, or text [default: json]
  -t, --timeout <TIMEOUT>        Request timeout in seconds [default: 30]
  -u, --user-agent <USER_AGENT>  Custom user agent
  -p, --proxy <PROXY>            Proxy URL (e.g., http://proxy.example.com:8080)
  -s, --selector <SELECTOR>      Custom CSS selector to extract (can specify multiple)
  -v, --verbose                  Enable verbose logging
  -q, --quiet                    Quiet mode (minimal output)
  -d, --delay <DELAY>            Delay between requests in milliseconds [default: 1000]
      --crawl                    Enable crawling (follow links)
      --max-depth <MAX_DEPTH>    Maximum crawl depth [default: 2]
      --max-pages <MAX_PAGES>    Maximum number of pages to crawl [default: 10]
      --metadata                 Extract metadata (Open Graph, meta tags)
  -o, --output <OUTPUT>          Save output to file
  -h, --help                     Print help
```

## Output Examples

### JSON Output
```json
[
  {
    "url": "https://books.toscrape.com",
    "status_code": 200,
    "title": "All products | Books to Scrape - Sandbox",
    "headings": [
      "All products",
      "A Light in the ...",
      "Tipping the Velvet",
      "Soumission"
    ],
    "paragraphs": [
      "£51.77",
      "In stock",
      "£53.74",
      "In stock"
    ],
    "links": [
      {
        "text": "Books to Scrape",
        "url": "https://books.toscrape.com/index.html"
      }
    ],
    "images": [
      {
        "alt": "A Light in the Attic",
        "src": "https://books.toscrape.com/media/cache/2c/da/2cdad67c44b002e7ead0cc35693c0e8b.jpg"
      }
    ],
    "custom_selectors": [
      {
        "selector": ".price_color",
        "matches": ["£51.77", "£53.74", "£50.10"]
      }
    ]
  }
]
```

### CSV Output
```csv
url,status_code,title,headings_count,paragraphs_count,links_count,images_count,depth
https://books.toscrape.com,200,All products | Books to Scrape - Sandbox,21,40,94,20,0
```

### Text Output
```
URL: https://books.toscrape.com
Status: 200
Title: All products | Books to Scrape - Sandbox

Headings (21):
  - All products
  - A Light in the ...
  - Tipping the Velvet
  ...

Paragraphs (40):
  1. £51.77
  2. In stock
  3. £53.74
  ...

Links (94):
  - Books to Scrape (https://books.toscrape.com/index.html)
  - A Light in the ... (https://books.toscrape.com/catalogue/a-light-in-the-attic_1000/index.html)
  ...

Custom Selectors:
  '.price_color' (20 matches):
    1. £51.77
    2. £53.74
    3. £50.10
```

## Project Structure

```
simple-web-scraper/
├── Cargo.toml          # Project dependencies
├── README.md           # This file
└── src/
    └── main.rs         # All scraper logic in ONE file! (~670 lines)
```

## Dependencies

Minimal but powerful:
- `tokio` - Async runtime
- `reqwest` - HTTP client with proxy support
- `scraper` - HTML parsing with CSS selectors
- `serde` / `serde_json` - JSON serialization
- `anyhow` / `thiserror` - Enhanced error handling
- `clap` - CLI argument parsing
- `csv` - CSV output support
- `log` / `env_logger` - Logging infrastructure
- `url` - URL parsing and manipulation
- `futures` - Async utilities

## Error Handling

The scraper provides helpful error messages for common issues:

- **Invalid URL**: Clear message about URL format problems
- **Timeout**: Indicates which request timed out and the timeout value
- **HTTP Errors**: Reports HTTP status codes and error details
- **Invalid Selectors**: Shows which CSS selector has syntax errors
- **Network Issues**: Detailed error information for connection problems

## Rate Limiting & Politeness

- Default 1-second delay between requests
- Configurable via `-d` flag
- Crawling respects the same domain (doesn't follow external links)
- Custom user-agent support to identify your bot

## Use Cases

- **Content Extraction**: Pull articles, products, or data from websites
- **SEO Analysis**: Extract metadata, headings, and link structures
- **Research**: Gather data from multiple pages automatically
- **Monitoring**: Track changes in website content
- **Data Mining**: Collect structured data for analysis
- **Testing**: Validate website structure and content

## Tips & Best Practices

1. **Start Small**: Test with a single URL before crawling
2. **Use Delays**: Respect server resources with appropriate delays (1-2 seconds)
3. **Set Timeouts**: Adjust timeout based on target website speed
4. **Custom Selectors**: Use browser DevTools to find the right CSS selectors
5. **Save Results**: Use `-o` to save large scraping jobs
6. **Verbose Mode**: Use `-v` for debugging selector or connection issues
7. **CSV for Analysis**: Use CSV format for easy import into Excel/spreadsheets

## Limitations

- Crawling only follows links within the same domain
- JavaScript-rendered content is not executed (static HTML only)
- No robots.txt parsing (respect websites' scraping policies manually)
- No parallel concurrent requests (sequential with delays)

## Comparison with Original Project

This is a simplified yet enhanced version of rust-web-scraper:
- ✅ **Single file** - Only 1 source file (main.rs)
- ✅ **No database** - Direct output to files or stdout
- ✅ **No API server** - Simple CLI interface
- ✅ **More features** - Crawling, metadata, custom selectors, multiple formats
- ✅ **Better errors** - Clear, actionable error messages
- ✅ **Easy to modify** - All code in one place, well-commented

Perfect for learning Rust web scraping or quick data extraction tasks!

## Contributing

This is designed to be a simple, single-file scraper. Feel free to fork and extend it for your needs!

## License

MIT License - Feel free to use in your projects!
