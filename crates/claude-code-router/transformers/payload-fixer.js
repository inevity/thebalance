// File: transformers/payload-fixer.js

class CloudflarePayloadFixer {
  constructor(options, logger) {
    this.name = "cloudflare-payload-fixer";
    this.log = logger || console.log;
  }

  transformRequestIn(request, provider) {
        this.log('[PayloadFixer] Transforming for provider:', provider?.name, 'Before:', JSON.stringify(request, null, 2));
    if (!request || !Array.isArray(request.messages)) {
      return request;
    }

    // Create a deep copy to avoid mutating the original object
    const newRequest = JSON.parse(JSON.stringify(request));

    newRequest.messages = newRequest.messages.filter((message, idx) => {
      // 打印所有消息用于调试
            this.log(`[PPPayloadFixer] Eval message[${idx}]:`, JSON.stringify(message));

      const hasToolCalls = Array.isArray(message.tool_calls) && message.tool_calls.length > 0;

      const hasValidContent = (() => {
        if (!Object.prototype.hasOwnProperty.call(message, "content")) {
          return false;
        }
        if (message.content === null || message.content === undefined) {
          return false;
        }
        if (typeof message.content === "string") {
          return message.content.trim() !== "";
        }
        if (Array.isArray(message.content)) {
          if (message.content.length === 0) return false;
          return message.content.some(part => {
            if (typeof part === "string") return part.trim() !== "";
            if (typeof part === "object" && part !== null) {
              if (part.type === "text" && part.text && part.text.trim() !== "") {
                return true;
              }
              if (part.type && part.type !== "text") {
                return true;
              }
            }
            return false;
          });
        }
        return true; // 非 null 非 undefined 的其他类型当作有效
      })();

      const isValid = hasToolCalls || hasValidContent;
            if (!isValid) {
        this.log(`[PayloadFixer] Dropping invalid message at index ${idx}:`, message);
      }

      return isValid;
    });

    // 若你使用 Gemini 格式，还可以保留原有逻辑
    if (newRequest.contents) {
      newRequest.contents = newRequest.contents.filter(contentObj => {
        return Array.isArray(contentObj.parts) && contentObj.parts.length > 0;
      });
    }

        this.log('[PayloadFixer] After transformation:', JSON.stringify(newRequest, null, 2));
    return newRequest;
  }

  // async transformResponseOut(response) {
  transformResponseOut(response) {
    // Check if this is a streaming response
    // if (response.body && response.body.getReader) {
    //   return this.fixStreamingToolCallCompletion(response);
    // }
    const contentType = response.headers.get('content-type') || '';
    const isEventStream = contentType.includes('text/event-stream');
    const isStreamingResponse = response.body && typeof response.body.getReader === 'function' && isEventStream;

    if (isStreamingResponse) {
      // if (response.body && typeof response.body.getReader === 'function') {
      const [logStream, processStream] = response.body.tee();
      
      // Log stream content asynchronously
      this.logStreamContent(logStream);
      
      // Continue processing with the other stream
      return this.fixStreamingToolCallCompletion(
        new Response(processStream, {
          status: response.status,
          statusText: response.statusText,
          headers: response.headers
        })
      );
    }

    
    // For non-streaming responses, return as-is
    return response;
  }

  async logStreamContent(stream) {
    const reader = stream.getReader();
    const decoder = new TextDecoder();
    
    try {
      let content = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        content += decoder.decode(value, { stream: true });
      }
            this.log('Complete stream content:', content);
    } catch (error) {
            this.log('Error logging stream:', error);
    } finally {
      reader.releaseLock();
    }
  }
  
  async fixStreamingToolCallCompletion(response) {
    const originalStream = response.body;
    const reader = originalStream.getReader();
    
    let hasToolCalls = false;
    let hasFinishReason = false;
    let lastModel = "unknown";
    let lastUsage = { prompt_tokens: 1, completion_tokens: 1 };
    const chunks = [];
    
    // Read and buffer the entire stream to analyze it
    try {
      const decoder = new TextDecoder();
      let buffer = "";
      
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        
        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split("\n");
        buffer = lines.pop() || "";
        
        for (const line of lines) {
          if (!line.startsWith("data: ")) continue;
          const data = line.slice(6);
          if (data === "[DONE]") continue;
          
          try {
            const chunk = JSON.parse(data);
            chunks.push(line + "\n"); // Store the original line
            
            // Track what we've seen
            if (chunk.model) lastModel = chunk.model;
            if (chunk.usage) lastUsage = chunk.usage;
            if (chunk.choices?.[0]?.delta?.tool_calls) hasToolCalls = true;
            if (chunk.choices?.[0]?.finish_reason) hasFinishReason = true;
            
          } catch (e) {
            chunks.push(line + "\n"); // Keep invalid chunks as-is
          }
        }
      }
      
      // Add remaining buffer
      if (buffer.trim()) {
        chunks.push(buffer);
      }
      
    } finally {
      reader.releaseLock();
    }
    
    // Fix: If we have tool calls but no finish_reason, add completion chunk
        if (hasToolCalls && !hasFinishReason) {
      this.log("Detected incomplete tool call stream, adding completion chunk");
      
      const completionChunk = {
        choices: [{
          finish_reason: "tool_calls",
          delta: {},
          index: 0
        }],
        usage: lastUsage,
        model: lastModel,
        created: Math.floor(Date.now() / 1000),
        id: `completion_${Date.now()}`,
        object: "chat.completion.chunk"
      };
      
      chunks.push(`data: ${JSON.stringify(completionChunk)}\n`);
    }
    
    // Always add [DONE] at the end
    chunks.push("data: [DONE]\n\n");
    
    // Create new stream with fixed content
    const fixedStream = new ReadableStream({
      start(controller) {
        const encoder = new TextEncoder();
        for (const chunk of chunks) {
          controller.enqueue(encoder.encode(chunk));
        }
        controller.close();
      }
    });
    
    // Return response with fixed stream
    return new Response(fixedStream, {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers
    });
  }

}

module.exports = CloudflarePayloadFixer;


