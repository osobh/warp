use super::*;

#[test]
fn test_access_record_creation() {
    let record = AccessRecord::new(ChunkId::new(42), 1000, EdgeIdx::new(5), 50);
    assert_eq!(record.chunk_id, ChunkId::new(42));
    assert_eq!(record.timestamp_ms, 1000);
    assert_eq!(record.edge_idx, EdgeIdx::new(5));
    assert_eq!(record.latency_ms, 50);
}

#[test]
fn test_access_pattern_sequential() {
    let pattern = AccessPattern::Sequential {
        start_chunk: ChunkId::new(10),
        count: 5,
        direction: 1,
    };
    match pattern {
        AccessPattern::Sequential {
            start_chunk,
            count,
            direction,
        } => {
            assert_eq!(start_chunk, ChunkId::new(10));
            assert_eq!(count, 5);
            assert_eq!(direction, 1);
        }
        _ => panic!("Expected Sequential pattern"),
    }
}

#[test]
fn test_access_pattern_hot() {
    let pattern = AccessPattern::Hot {
        chunk_id: ChunkId::new(100),
        access_count: 20,
    };
    match pattern {
        AccessPattern::Hot {
            chunk_id,
            access_count,
        } => {
            assert_eq!(chunk_id, ChunkId::new(100));
            assert_eq!(access_count, 20);
        }
        _ => panic!("Expected Hot pattern"),
    }
}

#[test]
fn test_pattern_config_defaults() {
    let config = PatternConfig::default();
    assert_eq!(config.window_size_ms, 60_000);
    assert_eq!(config.min_sequential_length, 5);
    assert_eq!(config.hot_threshold, 10);
    assert_eq!(config.max_records, 10_000);
}

#[test]
fn test_pattern_config_validation() {
    let config = PatternConfig {
        window_size_ms: 30_000,
        min_sequential_length: 3,
        hot_threshold: 5,
        max_records: 5_000,
    };
    assert_eq!(config.window_size_ms, 30_000);
    assert_eq!(config.min_sequential_length, 3);
    assert_eq!(config.hot_threshold, 5);
    assert_eq!(config.max_records, 5_000);
}

#[test]
fn test_pattern_detector_creation() {
    let config = PatternConfig::default();
    let detector = PatternDetector::new(config);
    assert_eq!(detector.records.len(), 0);
    assert_eq!(detector.access_counts.len(), 0);
}

#[test]
fn test_pattern_detector_record_access() {
    let mut detector = PatternDetector::new(PatternConfig::default());
    let record = AccessRecord::new(ChunkId::new(1), 1000, EdgeIdx::new(0), 10);
    detector.record_access(record);
    assert_eq!(detector.records.len(), 1);
    assert_eq!(detector.access_counts.get(&ChunkId::new(1)), Some(&1));
}

#[test]
fn test_pattern_detector_hot_chunk_detection() {
    let config = PatternConfig {
        hot_threshold: 3,
        ..Default::default()
    };
    let mut detector = PatternDetector::new(config);

    // Access same chunk 5 times
    for i in 0..5 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(42),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    let hot_chunks = detector.get_hot_chunks(3);
    assert_eq!(hot_chunks.len(), 1);
    assert_eq!(hot_chunks[0].0, ChunkId::new(42));
    assert_eq!(hot_chunks[0].1, 5);
}

#[test]
fn test_pattern_detector_sequential_forward() {
    let mut detector = PatternDetector::new(PatternConfig::default());

    // Sequential forward access
    for i in 0..10 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(100 + i),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    let runs = detector.get_sequential_runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, ChunkId::new(100));
    assert_eq!(runs[0].1, 10);
    assert_eq!(runs[0].2, 1);
}

#[test]
fn test_pattern_detector_sequential_backward() {
    let mut detector = PatternDetector::new(PatternConfig::default());

    // Sequential backward access
    for i in 0..8 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(200 - i),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    let runs = detector.get_sequential_runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, ChunkId::new(200));
    assert_eq!(runs[0].1, 8);
    assert_eq!(runs[0].2, -1);
}

#[test]
fn test_pattern_detector_pattern_clearing() {
    let config = PatternConfig {
        max_records: 5,
        ..Default::default()
    };
    let mut detector = PatternDetector::new(config);

    // Add 10 records
    for i in 0..10 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(i),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    // Should only keep last 5
    assert_eq!(detector.records.len(), 5);
    assert_eq!(detector.records.front().unwrap().chunk_id, ChunkId::new(5));
}

#[test]
fn test_pattern_detector_clear_old_records() {
    let mut detector = PatternDetector::new(PatternConfig::default());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    // Add old records
    for i in 0..5 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(i),
            now - 100_000 - i * 1000,
            EdgeIdx::new(0),
            10,
        ));
    }

    // Add recent records
    for i in 0..3 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(100 + i),
            now - i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    detector.clear_old_records(50_000);
    // Should only keep recent records
    assert_eq!(detector.records.len(), 3);
}

#[test]
fn test_preposition_request_creation() {
    let chunks = vec![ChunkId::new(1), ChunkId::new(2)];
    let edges = vec![EdgeIdx::new(0), EdgeIdx::new(1)];
    let request = PrepositionRequest::new(
        chunks.clone(),
        edges.clone(),
        PrepositionPriority::High,
        "test reason".to_string(),
    );

    assert_eq!(request.chunks, chunks);
    assert_eq!(request.target_edges, edges);
    assert_eq!(request.priority, PrepositionPriority::High);
    assert_eq!(request.reason, "test reason");
}

#[test]
fn test_preposition_priority_ordering() {
    assert!(PrepositionPriority::Critical > PrepositionPriority::High);
    assert!(PrepositionPriority::High > PrepositionPriority::Medium);
    assert!(PrepositionPriority::Medium > PrepositionPriority::Low);
}

#[test]
fn test_predictor_config_defaults() {
    let config = PredictorConfig::default();
    assert_eq!(config.lookahead_count, 10);
    assert_eq!(config.min_confidence, 0.6);
    assert_eq!(config.prefetch_threshold, 0.7);
    assert_eq!(config.max_preposition_batch, 50);
}

#[test]
fn test_predictor_creation() {
    let config = PredictorConfig::default();
    let predictor = Predictor::new(config);
    assert_eq!(predictor.recent_predictions.len(), 0);
}

#[test]
fn test_predictor_sequential_prediction() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Sequential {
        start_chunk: ChunkId::new(100),
        count: 5,
        direction: 1,
    }];

    let predictions = predictor.predict_next(&patterns);
    assert!(!predictions.is_empty());
    // Should predict chunks after 105 (100 + 5)
    assert_eq!(predictions[0], ChunkId::new(105));
}

#[test]
fn test_predictor_hot_chunk_prediction() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Hot {
        chunk_id: ChunkId::new(42),
        access_count: 20,
    }];

    let predictions = predictor.predict_next(&patterns);
    assert_eq!(predictions.len(), 1);
    assert_eq!(predictions[0], ChunkId::new(42));
}

#[test]
fn test_predictor_confidence_scoring() {
    let predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Hot {
        chunk_id: ChunkId::new(100),
        access_count: 15,
    }];

    let score = predictor.score_prediction(ChunkId::new(100), &patterns);
    assert!(score > 0.9);

    let score = predictor.score_prediction(ChunkId::new(200), &patterns);
    assert!(score < 0.1);
}

#[test]
fn test_predictor_prefetch_candidates() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    predictor.recent_predictions.insert(ChunkId::new(1), 0.9);
    predictor.recent_predictions.insert(ChunkId::new(2), 0.8);
    predictor.recent_predictions.insert(ChunkId::new(3), 0.5);

    let candidates = predictor.get_prefetch_candidates(2);
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0], ChunkId::new(1));
    assert_eq!(candidates[1], ChunkId::new(2));
}

#[test]
fn test_predictor_preposition_request_generation() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Sequential {
        start_chunk: ChunkId::new(50),
        count: 5,
        direction: 1,
    }];

    let predictions = predictor.predict_next(&patterns);
    let edges = vec![EdgeIdx::new(0), EdgeIdx::new(1)];
    let requests = predictor.generate_preposition_requests(&predictions, &edges);

    assert!(!requests.is_empty());
    assert!(!requests[0].chunks.is_empty());
    assert_eq!(requests[0].target_edges, edges);
}

#[test]
fn test_access_analytics_calculation() {
    let mut detector = PatternDetector::new(PatternConfig {
        hot_threshold: 3,
        ..Default::default()
    });

    // Add sequential accesses
    for i in 0..10 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(i),
            1000 + i * 100,
            EdgeIdx::new(0),
            20 + i,
        ));
    }

    let analytics = AccessAnalytics::from_detector(&detector);
    assert_eq!(analytics.total_accesses, 10);
    assert_eq!(analytics.unique_chunks, 10);
    assert!(analytics.avg_latency_ms > 0.0);
    assert!(analytics.sequential_ratio > 0.0);
}

#[test]
fn test_pattern_detector_detect_patterns() {
    let config = PatternConfig {
        hot_threshold: 3,
        min_sequential_length: 3,
        ..Default::default()
    };
    let mut detector = PatternDetector::new(config);

    // Add hot chunk accesses
    for i in 0..5 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(99),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    // Add sequential accesses
    for i in 0..5 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(200 + i),
            2000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    let patterns = detector.detect_patterns();
    assert!(!patterns.is_empty());

    let has_hot = patterns
        .iter()
        .any(|p| matches!(p, AccessPattern::Hot { .. }));
    let has_sequential = patterns
        .iter()
        .any(|p| matches!(p, AccessPattern::Sequential { .. }));
    assert!(has_hot);
    assert!(has_sequential);
}

#[test]
fn test_predictor_random_pattern_handling() {
    let predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Random {
        chunks: vec![ChunkId::new(1), ChunkId::new(5), ChunkId::new(10)],
    }];

    let score = predictor.score_prediction(ChunkId::new(5), &patterns);
    assert_eq!(score, 0.3);

    let score = predictor.score_prediction(ChunkId::new(100), &patterns);
    assert_eq!(score, 0.0);
}

#[test]
fn test_predictor_sequential_backward_prediction() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![AccessPattern::Sequential {
        start_chunk: ChunkId::new(100),
        count: 5,
        direction: -1,
    }];

    let predictions = predictor.predict_next(&patterns);
    assert!(!predictions.is_empty());
    // For backward direction, predicts from (100 - 5) = 95 going down
    assert_eq!(predictions[0], ChunkId::new(95));
}

#[test]
fn test_access_analytics_empty_detector() {
    let detector = PatternDetector::new(PatternConfig::default());
    let analytics = AccessAnalytics::from_detector(&detector);
    assert_eq!(analytics.total_accesses, 0);
    assert_eq!(analytics.unique_chunks, 0);
    assert_eq!(analytics.avg_latency_ms, 0.0);
    assert_eq!(analytics.sequential_ratio, 0.0);
    assert_eq!(analytics.hot_chunk_ratio, 0.0);
}

#[test]
fn test_integration_access_to_preposition() {
    // Complete flow: access -> pattern detection -> prediction -> preposition
    let mut detector = PatternDetector::new(PatternConfig {
        hot_threshold: 3,
        min_sequential_length: 3,
        ..Default::default()
    });

    // Simulate sequential access pattern
    for i in 0..10 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(100 + i),
            1000 + i * 100,
            EdgeIdx::new(0),
            15,
        ));
    }

    // Detect patterns
    let patterns = detector.detect_patterns();
    assert!(!patterns.is_empty());

    // Predict next accesses
    let mut predictor = Predictor::new(PredictorConfig::default());
    let predictions = predictor.predict_next(&patterns);
    assert!(!predictions.is_empty());

    // Generate preposition requests
    let edges = vec![EdgeIdx::new(0), EdgeIdx::new(1), EdgeIdx::new(2)];
    let requests = predictor.generate_preposition_requests(&predictions, &edges);
    assert!(!requests.is_empty());
    assert!(requests[0].priority >= PrepositionPriority::Low);
}

#[test]
fn test_predictor_batch_size_limit() {
    let config = PredictorConfig {
        max_preposition_batch: 5,
        ..Default::default()
    };
    let mut predictor = Predictor::new(config);

    let patterns = vec![AccessPattern::Sequential {
        start_chunk: ChunkId::new(0),
        count: 20,
        direction: 1,
    }];

    let predictions = predictor.predict_next(&patterns);
    let edges = vec![EdgeIdx::new(0)];
    let requests = predictor.generate_preposition_requests(&predictions, &edges);

    // Should split into multiple batches
    for request in &requests {
        assert!(request.chunks.len() <= 5);
    }
}

#[test]
fn test_pattern_detector_multiple_sequential_runs() {
    let mut detector = PatternDetector::new(PatternConfig::default());

    // First sequential run
    for i in 0..5 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(10 + i),
            1000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    // Gap
    detector.record_access(AccessRecord::new(
        ChunkId::new(100),
        2000,
        EdgeIdx::new(0),
        10,
    ));

    // Second sequential run
    for i in 0..7 {
        detector.record_access(AccessRecord::new(
            ChunkId::new(200 + i),
            3000 + i * 100,
            EdgeIdx::new(0),
            10,
        ));
    }

    let runs = detector.get_sequential_runs();
    assert!(runs.len() >= 2);
}

#[test]
fn test_predictor_empty_patterns() {
    let mut predictor = Predictor::new(PredictorConfig::default());
    let patterns = vec![];
    let predictions = predictor.predict_next(&patterns);
    assert!(predictions.is_empty());
}

#[test]
fn test_predictor_empty_edges() {
    let predictor = Predictor::new(PredictorConfig::default());
    let predictions = vec![ChunkId::new(1), ChunkId::new(2)];
    let edges = vec![];
    let requests = predictor.generate_preposition_requests(&predictions, &edges);
    assert!(requests.is_empty());
}
