use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, TextFieldIndexing, TextOptions};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use log::{info, error};
use tantivy::tokenizer::{TextAnalyzer, SimpleTokenizer, LowerCaser, Stemmer, Language, CJKTokenizer};

// Define the schema for our search index
pub struct SearchIndex {
    index: Index,
    pub input_content_field: Field,
    pub id_field: Field,
    index_writer: Arc<Mutex<IndexWriter>>,
}

impl SearchIndex {
    pub fn new(index_path: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut schema_builder = Schema::builder();

        // Text options for fields that will be searched
        let text_options = TextOptions::default()
            .set_indexing_options(TextFieldIndexing::default()
                .set_tokenizer("en_stemmed") // Use a tokenizer for better search
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions))
            .set_stored(); // Store the content so we can retrieve it

        let input_content_field = schema_builder.add_text_field("input_content", text_options.clone());
        let id_field = schema_builder.add_text_field("id", TextOptions::default().set_stored());

        let schema = schema_builder.build();

        // Create or open the index
        let index = Index::create_in_dir(&index_path, schema.clone())?;
        
        // CJK分词器
        let cjk_tokenizer = TextAnalyzer::builder(CJKTokenizer)
            .filter(LowerCaser)
            .build();
        index.tokenizers().register("cjk", cjk_tokenizer);

        let index_writer = index.writer(50_000_000)?; // 50 MB heap size for indexing
        let index_writer = Arc::new(Mutex::new(index_writer));

        info!("Tantivy search index initialized at {:?}", index_path);

        Ok(SearchIndex {
            index,
            input_content_field,
            id_field,
            index_writer,
        })
    }

    pub fn add_document(&self, doc: TantivyDocument) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut writer = self.index_writer.lock().unwrap();
        writer.add_document(doc)?;
        writer.commit()?; // Commit changes immediately for simplicity, consider batching for performance
        Ok(())
    }

    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<TantivyDocument>, Box<dyn std::error::Error + Send + Sync>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let query_parser = QueryParser::for_index(&self.index, vec![
            self.input_content_field,
        ]);

        let query = query_parser.parse_query(query_str)?;

        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc = searcher.doc(doc_address)?;
            results.push(retrieved_doc);
        }
        Ok(results)
    }
}

// Global static for the search index
use once_cell::sync::Lazy;
pub static SEARCH_INDEX: Lazy<Mutex<Option<SearchIndex>>> = Lazy::new(|| Mutex::new(None));

pub fn init_search_index(data_dir: PathBuf) {
    let index_path = data_dir.join("tantivy_index");
    match SearchIndex::new(index_path) {
        Ok(index) => {
            *SEARCH_INDEX.lock().unwrap() = Some(index);
            info!("Search index successfully initialized.");
        },
        Err(e) => {
            error!("Failed to initialize search index: {:?}", e);
        }
    }
}

pub fn get_search_index() -> Option<std::sync::MutexGuard<'static, Option<SearchIndex>>> {
    SEARCH_INDEX.lock().ok()
}
