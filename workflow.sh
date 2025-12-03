#!/bin/bash

# Combined Workflow Helper
# Integrates Memory Bank + Context Engineering

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

show_help() {
    echo "Combined AI Workflow Helper"
    echo ""
    echo "Usage: ./workflow.sh [command] [args]"
    echo ""
    echo "Commands:"
    echo "  start <task>     Start new task (updates both systems)"
    echo "  research <task>  Begin research phase"
    echo "  plan <task>      Create implementation plan"
    echo "  implement        Begin implementation"
    echo "  update           Update memory bank from current work"
    echo "  status           Show current status"
    echo "  complete <task>  Complete task and archive"
}

start_task() {
    TASK="$1"
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    
    echo -e "${BLUE}Starting task: $TASK${NC}"
    
    # Update Memory Bank
    echo -e "\n## Task Started: $TASK ($TIMESTAMP)" >> memory-bank/activeContext.md
    
    # Create Context Engineering files
    cp .ai/prompts/research.md ".ai/research/${TIMESTAMP}_${TASK}.md"
    cp .ai/prompts/plan.md ".ai/plans/${TIMESTAMP}_${TASK}.md"
    cp .ai/progress/template.md .ai/progress/current.md
    
    echo -e "${GREEN}✓ Task initialized in both systems${NC}"
    echo "Next: Begin research phase"
}

update_memory_bank() {
    echo -e "${BLUE}Updating Memory Bank from current work...${NC}"
    
    # This would be done by AI, but showing the intent
    echo -e "\n## Updated: $(date)" >> memory-bank/activeContext.md
    echo "Review all memory-bank files and update based on current progress"
    
    echo -e "${GREEN}✓ Memory Bank update requested${NC}"
}

complete_task() {
    TASK="$1"
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    
    echo -e "${BLUE}Completing task: $TASK${NC}"
    
    # Archive Context Engineering files
    mkdir -p ".ai/archive/${TASK}_${TIMESTAMP}"
    mv .ai/research/*${TASK}* ".ai/archive/${TASK}_${TIMESTAMP}/" 2>/dev/null || true
    mv .ai/plans/*${TASK}* ".ai/archive/${TASK}_${TIMESTAMP}/" 2>/dev/null || true
    cp .ai/progress/current.md ".ai/archive/${TASK}_${TIMESTAMP}/final_progress.md"
    
    # Update Memory Bank
    echo -e "\n## Completed: $TASK ($(date))" >> memory-bank/progress.md
    
    echo -e "${GREEN}✓ Task completed and archived${NC}"
}

status() {
    echo -e "${BLUE}Current Workflow Status${NC}"
    echo ""
    
    if [ -f "memory-bank/activeContext.md" ]; then
        echo -e "${YELLOW}Active Context:${NC}"
        tail -5 memory-bank/activeContext.md | head -3
        echo ""
    fi
    
    if [ -f ".ai/progress/current.md" ]; then
        echo -e "${YELLOW}Current Task Progress:${NC}"
        grep -E "^## Objective|^## Current Status" .ai/progress/current.md || echo "No active task"
        echo ""
    fi
    
    echo -e "${YELLOW}Recent Files:${NC}"
    ls -t .ai/research/ 2>/dev/null | head -2 | sed 's/^/  Research: /'
    ls -t .ai/plans/ 2>/dev/null | head -2 | sed 's/^/  Plans: /'
}

case "$1" in
    start) start_task "$2" ;;
    update) update_memory_bank ;;
    complete) complete_task "$2" ;;
    status) status ;;
    *) show_help ;;
esac
