# Combined AI Development Workflow

This project uses two complementary AI assistance systems:

## Memory Bank (memory-bank/)
Persistent documentation that maintains project context across sessions:
- Project overview and goals
- Technical decisions and patterns
- Current status and progress

## Context Engineering (.ai/)
Structured workflow for executing complex tasks:
- Research → Plan → Implement
- 40% context rule for quality
- Detailed documentation of all work

## Quick Start

### New Task
```bash
./workflow.sh start "implement-auth"
# Then in AI: "Begin research phase for implement-auth"
```

### Update Project Memory
```bash
./workflow.sh update
# Then in AI: "Update memory bank with current progress"
```

### Complete Task
```bash
./workflow.sh complete "implement-auth"
```

### Check Status
```bash
./workflow.sh status
```

## Workflow

1. AI loads memory-bank files (project context)
2. Research phase (.ai/research/)
3. Planning phase (.ai/plans/)
4. Implementation with 40% context rule
5. Update memory-bank with results
6. Archive task files

## Key Principles

- Memory Bank = What your project IS
- Context Engineering = How you WORK
- 40% Rule = Maintain quality
- Document Everything = Nothing gets lost
