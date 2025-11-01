# Example Outputs

This directory contains example output files from scraping https://books.toscrape.com - a website designed for practicing web scraping.

## Files

### books-basic.json
Basic scraping output in JSON format with custom selectors for book titles and prices.

**Command used:**
```bash
cargo run --release -- https://books.toscrape.com \
  -s "article.product_pod h3 a" \
  -s ".price_color" \
  -o examples/books-basic.json
```

**Contains:**
- URL and status code
- Page title
- Headings (book titles)
- Paragraphs (prices and stock info)
- Links to book detail pages
- Images (book covers)
- Custom selector results (titles and prices extracted separately)

### books-basic.csv
Basic scraping output in CSV format, perfect for importing into Excel or Google Sheets.

**Command used:**
```bash
cargo run --release -- https://books.toscrape.com \
  --format csv \
  -o examples/books-basic.csv
```

**Columns:**
- url
- status_code
- title
- headings_count
- paragraphs_count
- links_count
- images_count
- depth

### books-basic.txt
Human-readable text format output showing all scraped data.

**Command used:**
```bash
cargo run --release -- https://books.toscrape.com \
  --format text \
  -o examples/books-basic.txt
```

**Contains:**
- URL and status
- Page title
- List of headings
- Paragraphs (with preview)
- Links (with preview)
- Images

### books-crawl.csv
CSV output from crawling the website, visiting multiple pages across different categories.

**Command used:**
```bash
cargo run --release -- https://books.toscrape.com \
  --crawl \
  --max-pages 10 \
  --format csv \
  -o examples/books-crawl.csv
```

**Contains:**
- Data from 10 different pages
- Different book categories (Travel, Mystery, Historical Fiction, etc.)
- Depth information showing how far from the starting page each URL was found

## Reproducing These Examples

You can regenerate these examples or create your own by running the commands shown above. Try different options:

```bash
# Add metadata extraction
cargo run --release -- https://books.toscrape.com --metadata -o with-metadata.json

# Crawl more pages
cargo run --release -- https://books.toscrape.com --crawl --max-pages 50 -o large-crawl.json

# Extract specific data points
cargo run --release -- https://books.toscrape.com \
  -s ".star-rating" \
  -s ".price_color" \
  -s ".instock" \
  --format csv -o book-ratings.csv
```

## About books.toscrape.com

Books to Scrape (https://books.toscrape.com) is a sandbox website specifically designed for practicing web scraping. It's safe to scrape and perfect for testing and learning.

**Features:**
- 1000 fictional books
- Multiple categories
- Book ratings, prices, and availability
- Pagination
- Clean, consistent HTML structure
- No rate limiting or IP blocking
