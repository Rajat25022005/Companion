import { useState, useRef, useEffect } from 'react'
import './App.css'

const API = '/api'

function FileAttachment({ url, name }) {
  const ext = name?.split('.').pop()?.toLowerCase() || ''
  const isImage = ['png', 'jpg', 'jpeg', 'svg'].includes(ext)

  if (isImage) {
    return (
      <div className="file-attachment">
        <img src={`${API}${url}`} alt={name} className="attachment-image" />
        <a href={`${API}${url}`} download={name} className="download-link">
          Download {name}
        </a>
      </div>
    )
  }

  return (
    <div className="file-attachment">
      <a href={`${API}${url}`} download={name} className="download-link">
        📎 {name}
      </a>
    </div>
  )
}

function parseFiles(text) {
  const filePattern = /\[FILE:([^\]]+)\]\(([^)]+)\)/g
  const urlPattern = /\/files\/[\w.-]+/g

  const files = []
  let match

  while ((match = filePattern.exec(text)) !== null) {
    files.push({ name: match[1], url: match[2] })
  }

  if (files.length === 0) {
    while ((match = urlPattern.exec(text)) !== null) {
      const url = match[0]
      const name = url.split('/').pop()
      if (!files.some(f => f.url === url)) {
        files.push({ name, url })
      }
    }
  }

  return files
}

function App() {
  const [messages, setMessages] = useState([])
  const [input, setInput] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(null)
  const messagesEndRef = useRef(null)
  const inputRef = useRef(null)

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, loading])

  useEffect(() => {
    inputRef.current?.focus()
  }, [loading])

  const sendMessage = async () => {
    const text = input.trim()
    if (!text || loading) return

    setInput('')
    setError(null)
    setMessages(prev => [...prev, { role: 'user', content: text }])
    setLoading(true)

    try {
      const res = await fetch(`${API}/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: text }),
      })

      if (!res.ok) {
        const data = await res.json().catch(() => ({}))
        throw new Error(data.detail || `HTTP ${res.status}`)
      }

      const data = await res.json()
      const files = parseFiles(data.content)

      setMessages(prev => [...prev, {
        role: 'assistant',
        content: data.content,
        intent: data.intent,
        latency_ms: data.latency_ms,
        files,
      }])
    } catch (err) {
      setError(err.message === 'Failed to fetch'
        ? 'Cannot connect to API. Run: python main.py'
        : err.message
      )
    } finally {
      setLoading(false)
    }
  }

  const resetSession = async () => {
    try {
      await fetch(`${API}/reset`, { method: 'POST' })
      setMessages([])
      setError(null)
    } catch {
      setError('Cannot connect to API.')
    }
  }

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      sendMessage()
    }
  }

  return (
    <div className="app">
      <div className="header">
        <h1>Companion</h1>
        <div className="header-actions">
          <button onClick={resetSession}>New Session</button>
        </div>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="messages">
        {messages.length === 0 && !loading && (
          <div className="empty-state">What are you working on?</div>
        )}

        {messages.map((msg, i) => (
          <div key={i} className={`message ${msg.role}`}>
            {msg.content}

            {msg.files?.length > 0 && (
              <div className="files-container">
                {msg.files.map((f, j) => (
                  <FileAttachment key={j} url={f.url} name={f.name} />
                ))}
              </div>
            )}

            {msg.role === 'assistant' && (msg.intent || msg.latency_ms) && (
              <div className="message-meta">
                {msg.intent && <span className="intent-badge">{msg.intent}</span>}
                {msg.latency_ms && <span>{Math.round(msg.latency_ms)}ms</span>}
              </div>
            )}
          </div>
        ))}

        {loading && (
          <div className="message assistant loading">Thinking...</div>
        )}

        <div ref={messagesEndRef} />
      </div>

      <div className="input-area">
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type a message..."
          disabled={loading}
        />
        <button onClick={sendMessage} disabled={loading || !input.trim()}>
          Send
        </button>
      </div>
    </div>
  )
}

export default App
