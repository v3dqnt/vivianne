export interface ContextBudget {
	totalTokens: number;
	systemTokens: number;
	userTokens: number;
	memoryTokens: number; // History
	retrievalTokens: number; // RAG
}

export function allocateContextBudget(maxTokens: number, queryComplexity: number): ContextBudget {
	// System prompt is fixed size initially, might grow with context
	const systemTokens = 1000;
	const remaining = Math.max(0, maxTokens - systemTokens);

	// Dynamic allocation based on query complexity
	// High complexity = more RAG needed; Low complexity = more dialogue history
	const retrievalRatio = queryComplexity > 0.7 ? 0.6 : 0.3;

	return {
		totalTokens: maxTokens,
		systemTokens,
		retrievalTokens: Math.floor(remaining * retrievalRatio),
		memoryTokens: Math.floor(remaining * (0.9 - retrievalRatio)),
		userTokens: Math.floor(remaining * 0.1),
	};
}
