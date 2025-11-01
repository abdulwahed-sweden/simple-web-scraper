# Usage Examples

This file contains practical examples demonstrating the various features of simple-web-scraper.

## Table of Contents
- [Basic Scraping](#basic-scraping)
- [Output Formats](#output-formats)
- [Custom Selectors](#custom-selectors)
- [Metadata Extraction](#metadata-extraction)
- [Web Crawling](#web-crawling)
- [Advanced Scenarios](#advanced-scenarios)

## Basic Scraping

### Scrape a single page
```bash
cargo run --release -- https://books.toscrape.com
```

### Scrape multiple pages
```bash
cargo run --release -- \
  https://books.toscrape.com \
  https://quotes.toscrape.com \
  https://books.toscrape.com/catalogue/category/books/travel_2/index.html
```

### Save output to a file
```bash
cargo run --release -- https://books.toscrape.com -o books-data.json
```

## Output Formats

### JSON (Default)
```bash
cargo run --release -- https://books.toscrape.com
```

Output:
```json
[
  {
    "url": "https://books.toscrape.com",
    "status_code": 200,
    "title": "All products | Books to Scrape - Sandbox",
    "headings": ["All products", "A Light in the ...", ...],
    "paragraphs": ["£51.77", "In stock", ...],
    "links": [...],
    "images": [...]
  }
]
```

### CSV Format
```bash
cargo run --release -- https://books.toscrape.com --format csv
```

Perfect for importing into Excel or Google Sheets!

### Plain Text Format
```bash
cargo run --release -- https://books.toscrape.com --format text
```

Great for human-readable output.

## Custom Selectors

### Extract specific elements
```bash
# Extract all book titles
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a"

# Extract book prices
cargo run --release -- https://books.toscrape.com -s ".price_color"

# Extract multiple different elements (titles, prices, ratings)
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -s ".star-rating" \
  -s ".instock"
```

### Export custom data to CSV
```bash
cargo run --release -- https://books.toscrape.com \
  -s "h3 a" \
  -s ".price_color" \
  --format csv -o books-prices.csv
```

## Metadata Extraction

### Extract Open Graph and meta tags
```bash
cargo run --release -- https://books.toscrape.com --metadata
```

This extracts:
- Page description
- Keywords
- Author
- Open Graph title
- Open Graph description
- Open Graph image
- Canonical URL
- Favicon

### Metadata with text output
```bash
cargo run --release -- https://books.toscrape.com --metadata --format text
```

## Web Crawling

### Basic crawl (default: depth 2, max 10 pages)
```bash
cargo run --release -- https://books.toscrape.com --crawl
```

### Deep crawl with custom limits
```bash
# Crawl up to 50 pages, 3 levels deep
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-depth 3 \
  --max-pages 50
```

### Crawl and extract metadata
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --metadata \
  --max-pages 30 \
  -o books-catalog.json
```

### Slow, polite crawl
```bash
# 2-second delay between requests
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --delay 2000 \
  --max-pages 30
```

## Advanced Scenarios

### Complete catalog scrape
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --metadata \
  --max-depth 2 \
  --max-pages 100 \
  --delay 1000 \
  -o complete-catalog.json
```

### Monitor book prices across categories
```bash
cargo run --release -- \
  https://books.toscrape.com/catalogue/category/books/travel_2/index.html \
  https://books.toscrape.com/catalogue/category/books/mystery_3/index.html \
  https://books.toscrape.com/catalogue/category/books/historical-fiction_4/index.html \
  -s "h3 a" \
  -s ".price_color" \
  -s ".instock" \
  --format csv \
  -o book-prices-$(date +%Y%m%d).csv
```

### Extract all book data from a category
```bash
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -s ".star-rating" \
  -s ".instock" \
  --metadata \
  --format json \
  -o books-with-ratings.json
```

### Use with proxy
```bash
cargo run --release -- https://books.toscrape.com \
  -p http://proxy.example.com:8080 \
  -u "BookScraperBot/1.0"
```

### Verbose debugging
```bash
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -v
```

### Quiet batch processing
```bash
# Process multiple book category URLs silently
for category in travel mystery fiction; do
  cargo run --release -- \
    "https://books.toscrape.com/catalogue/category/books/${category}_*/index.html" \
    -q -o "${category}-books.json"
done
```

### Timeout configuration
```bash
# 60-second timeout for slower connections
cargo run --release -- https://books.toscrape.com -t 60
```

### Combined example: Full book data extraction
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-depth 2 \
  --max-pages 50 \
  --metadata \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -s ".star-rating" \
  --delay 1000 \
  --timeout 45 \
  --format json \
  -o books-complete-data.json
```

## Error Handling Examples

### Handle invalid URLs gracefully
```bash
# The scraper will show a clear error message
cargo run --release -- "not-a-valid-url"
```

### Handle timeouts
```bash
# Will timeout after 5 seconds (useful for testing)
cargo run --release -- https://books.toscrape.com -t 5
```

### Verbose mode for debugging
```bash
# See detailed logs of what's happening
cargo run --release -- https://books.toscrape.com -v -s "article.product_pod"
```

## Tips

1. **Start small**: Always test with a single URL first
2. **Use delays**: Be respectful to servers with appropriate delays
3. **Save results**: Use `-o` for large scraping jobs
4. **Validate selectors**: Use browser DevTools to test CSS selectors first
5. **Monitor progress**: Use verbose mode (`-v`) to see what's happening
6. **CSV for analysis**: Use CSV format for easy data analysis in Excel
7. **Combine features**: Mix crawling, metadata, and custom selectors for powerful data extraction

## Common Use Cases

### 1. Book Price Monitoring
```bash
# Monitor prices across all categories
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-pages 50 \
  -s ".price_color" \
  --format csv \
  -o book-prices-$(date +%Y%m%d).csv
```

### 2. Catalog Aggregation
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-pages 100 \
  --metadata \
  -o complete-catalog.json
```

### 3. Category Analysis
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-depth 2 \
  --format json \
  -o category-structure.json
```

### 4. Book Data Extraction
```bash
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -s ".star-rating" \
  --format csv \
  -o books-with-ratings.csv
```

### 5. Multi-Category Comparison
```bash
# Compare books across different categories
for category in travel_2 mystery_3 fiction_10; do
  cargo run --release -- \
    "https://books.toscrape.com/catalogue/category/books/$category/index.html" \
    -s "h3 a" -s ".price_color" \
    -o "category-$category.json"
done
```
