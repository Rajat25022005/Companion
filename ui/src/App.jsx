import { useState, useRef, useEffect, Component } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter'
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism'
import './App.css'
import { RepoSelector } from './RepoSelector'

const API = '/api'

// ── Error boundary ────────────────────────────────────────────────────────────
class MarkdownBoundary extends Component {
  constructor(props) {
    super(props)
    this.state = { hasError: false }
  }
  static getDerivedStateFromError() { return { hasError: true } }
  render() {
    if (this.state.hasError) return <pre className="message-text">{this.props.fallback}</pre>
    return this.props.children
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────
function CodeBlock({ className, children, ...props }) {
  const match = /language-(\w+)/.exec(className || '')
  const codeStr = String(children).replace(/\n$/, '')
  if (match) {
    return (
      <div className="code-wrapper">
        <div className="code-header">
          <span>{match[1]}</span>
          <button onClick={() => navigator.clipboard.writeText(codeStr)}>copy</button>
        </div>
        <SyntaxHighlighter
          style={vscDarkPlus}
          language={match[1]}
          PreTag="div"
          customStyle={{ margin: 0, background: 'transparent', padding: '16px', fontSize: '12px' }}
        >
          {codeStr}
        </SyntaxHighlighter>
      </div>
    )
  }
  return <code className={className} {...props}>{children}</code>
}

const MD_COMPONENTS = { code: CodeBlock }

function FileAttachment({ url, name }) {
  const ext = name?.split('.').pop()?.toLowerCase() || ''
  const isImage = ['png', 'jpg', 'jpeg', 'svg'].includes(ext)
  if (isImage) {
    return (
      <div className="file-attachment">
        <img src={`${API}${url}`} alt={name} />
        <a href={`${API}${url}`} download={name} className="download-link">DOWNLOAD {name}</a>
      </div>
    )
  }
  return (
    <div className="file-attachment">
      <a href={`${API}${url}`} download={name} className="download-link">FILE: {name}</a>
    </div>
  )
}

function parseFiles(text) {
  if (!text) return []
  const files = []
  let m
  const re1 = /\[FILE:([^\]]+)\]\(([^)]+)\)/g
  while ((m = re1.exec(text)) !== null) files.push({ name: m[1], url: m[2] })
  if (!files.length) {
    const re2 = /\/files\/[\w.-]+/g
    while ((m = re2.exec(text)) !== null) {
      const url = m[0]; const name = url.split('/').pop()
      if (!files.some(f => f.url === url)) files.push({ name, url })
    }
  }
  return files
}

function AgentPipeline({ events }) {
  if (!events?.length) return null
  const agents = {}
  events.forEach(e => {
    if (['agent_start', 'agent_done', 'agent_error'].includes(e.type)) {
      agents[e.step] = {
        agent: e.agent,
        status: e.type === 'agent_start' ? 'running' : e.type === 'agent_done' ? 'done' : 'error',
        latency_ms: e.latency_ms,
      }
    }
  })
  const steps = Object.keys(agents).sort((a, b) => +a - +b)
  if (!steps.length) return null
  return (
    <div className="agent-pipeline">
      <span className="pipeline-label">pipeline</span>
      {steps.map((step, idx) => {
        const d = agents[step]
        return (
          <div key={step} className="pipeline-step">
            <span className={`step-dot ${d.status}`} />
            <span className="step-name">{d.agent}</span>
            {d.latency_ms != null && <span className="step-time">{(d.latency_ms / 1000).toFixed(1)}s</span>}
            {idx < steps.length - 1 && <span className="step-arrow">–</span>}
          </div>
        )
      })}
    </div>
  )
}

function MessageContent({ msg }) {
  if (msg.isStreaming && !msg.content) {
    return (
      <div className="streaming-indicator">
        <span className="dot" /><span className="dot" /><span className="dot" />
      </div>
    )
  }
  if (msg.role === 'user') return <div className="message-text">{msg.content}</div>
  const content = typeof msg.content === 'string' ? msg.content.trim() : ''
  if (!content) return null
  return (
    <MarkdownBoundary fallback={content}>
      <div className="markdown-body">
        <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
          {content}
        </ReactMarkdown>
      </div>
    </MarkdownBoundary>
  )
}

function formatDate(ts) {
  if (!ts) return ''
  const d = new Date(ts * 1000)
  const now = new Date()
  const isToday = d.toDateString() === now.toDateString()
  const yesterday = new Date(now); yesterday.setDate(now.getDate() - 1)
  const isYesterday = d.toDateString() === yesterday.toDateString()
  if (isToday) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  if (isYesterday) return 'Yesterday'
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' })
}

// ── Sidebar ───────────────────────────────────────────────────────────────────
function Sidebar({ sessions, activeId, onSelect, onNew, onDelete }) {
  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Sessions</span>
        <button className="sidebar-new" onClick={onNew} title="New session">+</button>
      </div>
      <div className="sidebar-list">
        {sessions.length === 0 && (
          <div className="sidebar-empty">No saved sessions</div>
        )}
        {sessions.map(s => (
          <div
            key={s.id}
            className={`sidebar-item ${s.id === activeId ? 'active' : ''}`}
            onClick={() => onSelect(s.id)}
          >
            <div className="sidebar-item-title">{s.title || 'Untitled'}</div>
            <div className="sidebar-item-meta">
              <span>{formatDate(s.updated_at)}</span>
              <span>{s.turn_index} turns</span>
            </div>
            <button
              className="sidebar-delete"
              onClick={e => { e.stopPropagation(); onDelete(s.id) }}
              title="Delete"
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── Main App ──────────────────────────────────────────────────────────────────
function App() {
  const [messages, setMessages] = useState([])
  const [input, setInput] = useState('')
  const [error, setError] = useState(null)
  const [sessions, setSessions] = useState([])
  const [activeSessionId, setActiveSessionId] = useState(null)
  const [sidebarOpen, setSidebarOpen] = useState(true)
  const [stagedFiles, setStagedFiles] = useState([])
  const [isUploading, setIsUploading] = useState(false)
  const [showRepoSelector, setShowRepoSelector] = useState(false)

  const messagesEndRef = useRef(null)
  const inputRef = useRef(null)
  const fileInputRef = useRef(null)
  const placeholderIdxRef = useRef(-1)
  const activeSessionIdRef = useRef(null)

  // Keep ref in sync with state (for use inside closures)
  useEffect(() => { activeSessionIdRef.current = activeSessionId }, [activeSessionId])

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  useEffect(() => {
    inputRef.current?.focus()
    fetchSessions()
  }, [])

  const fetchSessions = async () => {
    try {
      const res = await fetch(`${API}/sessions`)
      if (res.ok) {
        const data = await res.json()
        setSessions(data.sessions || [])
      }
    } catch { /* no-op */ }
  }

  // Auto-save current session after messages update (debounced)
  const saveTimerRef = useRef(null)
  const saveSession = (msgs) => {
    const sid = activeSessionIdRef.current
    if (!sid || !msgs.some(m => m.role === 'user')) return
    clearTimeout(saveTimerRef.current)
    saveTimerRef.current = setTimeout(async () => {
      try {
        // Strip non-serialisable fields (isStreaming, events)
        const clean = msgs.map(m => ({
          role: m.role,
          content: m.content || '',
          intent: m.intent,
          latency_ms: m.latency_ms,
          files: m.files,
        }))
        await fetch(`${API}/sessions/${sid}/save`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ messages: clean }),
        })
        fetchSessions()
      } catch { /* no-op */ }
    }, 1000)
  }

  const updateMessage = (idx, updater) => {
    setMessages(prev => {
      const next = [...prev]
      if (idx >= 0 && idx < next.length) next[idx] = updater({ ...next[idx] })
      // Trigger save after every update
      saveSession(next)
      return next
    })
  }

  const handleTaskStream = (taskId, msgIdx) => {
    const es = new EventSource(`${API}/tasks/${taskId}/stream`)
    let errCount = 0
    es.onmessage = (e) => {
      let data; try { data = JSON.parse(e.data) } catch { return }
      updateMessage(msgIdx, msg => {
        const events = [...(msg.events || []), data]
        if (data.type === 'complete') {
          es.close()
          return { ...msg, events, content: data.content || '', intent: data.intent,
            latency_ms: data.latency_ms, files: parseFiles(data.content), isStreaming: false }
        }
        if (data.type === 'error') {
          es.close()
          return { ...msg, events, content: `Error: ${data.error}`, isStreaming: false }
        }
        return { ...msg, events }
      })
    }
    es.onerror = () => {
      errCount++
      if (errCount > 5) {
        es.close()
        updateMessage(msgIdx, msg => ({
          ...msg, isStreaming: false, content: msg.content || 'Connection lost.',
        }))
      }
    }
  }

  const handleFileUpload = async (e) => {
    const file = e.target.files?.[0]
    if (!file) return
    
    setIsUploading(true)
    setError(null)
    
    const formData = new FormData()
    formData.append('file', file)
    
    try {
      const res = await fetch(`${API}/upload`, {
        method: 'POST',
        body: formData,
      })
      
      if (!res.ok) throw new Error('Upload failed')
      
      const data = await res.json()
      setStagedFiles(prev => [...prev, data])
    } catch (err) {
      setError('File upload failed: ' + err.message)
    } finally {
      setIsUploading(false)
      if (fileInputRef.current) fileInputRef.current.value = ''
    }
  }

  const sendMessage = async () => {
    let text = input.trim()
    if (!text && stagedFiles.length === 0) return
    
    // Append staged files to the text prompt
    if (stagedFiles.length > 0) {
      const fileRefs = stagedFiles.map(f => `[FILE:${f.filename}](${f.path})`).join('\n')
      text = text ? `${text}\n\n${fileRefs}` : fileRefs
    }

    setInput('')
    setStagedFiles([])
    setError(null)

    // If no active session, create one via /reset to get a new session_id from backend
    let sid = activeSessionIdRef.current
    if (!sid) {
      try {
        const res = await fetch(`${API}/reset`, { method: 'POST' })
        const data = await res.json()
        sid = data.session_id
        setActiveSessionId(sid)
      } catch { /* use whatever session the backend has */ }
    }

    setMessages(prev => {
      placeholderIdxRef.current = prev.length + 1
      return [
        ...prev,
        { role: 'user', content: text },
        { role: 'assistant', content: '', isStreaming: true, events: [] },
      ]
    })

    await new Promise(r => setTimeout(r, 0))
    const msgIdx = placeholderIdxRef.current

    try {
      const res = await fetch(`${API}/chat/async`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: text }),
      })
      if (!res.ok) {
        const d = await res.json().catch(() => ({}))
        throw new Error(d.detail || `HTTP ${res.status}`)
      }
      const { task_id } = await res.json()
      handleTaskStream(task_id, msgIdx)
    } catch (err) {
      setError(err.message === 'Failed to fetch'
        ? 'Cannot connect to API — run python main.py'
        : err.message)
      updateMessage(msgIdx, msg => ({ ...msg, isStreaming: false, content: 'Request failed.' }))
    }
  }

  const loadSession = async (sessionId) => {
    try {
      const res = await fetch(`${API}/sessions/${sessionId}/load`, { method: 'POST' })
      if (!res.ok) throw new Error('Load failed')
      const data = await res.json()
      setActiveSessionId(data.session_id)
      // Restore UI messages
      const restored = (data.messages || []).map(m => ({
        role: m.role,
        content: m.content || '',
        intent: m.intent,
        latency_ms: m.latency_ms,
        files: m.files || [],
        events: [],
      }))
      setMessages(restored)
      setError(null)
    } catch {
      setError('Could not load session.')
    }
  }

  const newSession = async () => {
    try {
      const res = await fetch(`${API}/reset`, { method: 'POST' })
      const data = await res.json()
      setActiveSessionId(data.session_id)
      setMessages([])
      setError(null)
      fetchSessions()
    } catch {
      setError('Cannot connect to API.')
    }
  }

  const deleteSession = async (sessionId) => {
    await fetch(`${API}/sessions/${sessionId}`, { method: 'DELETE' })
    if (sessionId === activeSessionId) {
      setMessages([])
      setActiveSessionId(null)
    }
    fetchSessions()
  }

  const handleKeyDown = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMessage() }
  }

  return (
    <div className="app">
      {/* Sidebar */}
      {sidebarOpen && (
        <Sidebar
          sessions={sessions}
          activeId={activeSessionId}
          onSelect={loadSession}
          onNew={newSession}
          onDelete={deleteSession}
        />
      )}

      {/* Main chat */}
      <div className="main">
        <div className="header">
          <div className="header-left">
            <button className="sidebar-toggle" onClick={() => setSidebarOpen(o => !o)}>
              {sidebarOpen ? '←' : '☰'}
            </button>
            <h1>Companion</h1>
          </div>
          <div className="header-actions">
            <button onClick={() => setShowRepoSelector(true)}>Index Repo</button>
            <button onClick={newSession}>New Session</button>
          </div>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="messages">
          {messages.length === 0 && (
            <div className="empty-state">system ready</div>
          )}
          {messages.map((msg, i) => (
            <div key={i} className={`message ${msg.role}`}>
              <span className="message-label">{msg.role === 'user' ? 'you' : 'companion'}</span>
              <div className="message-content">
                {msg.role === 'assistant' && msg.events?.length > 0 && (
                  <AgentPipeline events={msg.events} />
                )}
                <MessageContent msg={msg} />
                {msg.files?.length > 0 && (
                  <div className="files-container">
                    {msg.files.map((f, j) => <FileAttachment key={j} url={f.url} name={f.name} />)}
                  </div>
                )}
              </div>
              {msg.role === 'assistant' && (msg.intent || msg.latency_ms != null) && (
                <div className="message-meta">
                  {msg.intent && <span className="intent-badge">{msg.intent}</span>}
                  {msg.latency_ms != null && <span>{(msg.latency_ms / 1000).toFixed(1)}s</span>}
                </div>
              )}
            </div>
          ))}
          <div ref={messagesEndRef} />
        </div>

        <div className="input-area">
          {stagedFiles.length > 0 && (
            <div className="staged-files">
              {stagedFiles.map((f, i) => (
                <span key={i} className="staged-file-badge">
                  {f.filename} {f.indexed ? '✓' : ''}
                </span>
              ))}
            </div>
          )}
          <div className="input-wrapper">
            <input 
              type="file" 
              ref={fileInputRef} 
              style={{ display: 'none' }} 
              onChange={handleFileUpload} 
            />
            <button 
              className="attach-button" 
              onClick={() => fileInputRef.current?.click()}
              title="Attach file"
              disabled={isUploading}
            >
              {isUploading ? '…' : '+'}
            </button>
            <textarea
              ref={inputRef}
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="enter command..."
              rows={1}
            />
            <button onClick={sendMessage} disabled={(!input.trim() && stagedFiles.length === 0) || isUploading}>
              Send
            </button>
          </div>
        </div>
      </div>
      {showRepoSelector && (
        <RepoSelector
          onClose={() => setShowRepoSelector(false)}
          onIndexed={(repoName, stats) => {
            setMessages(prev => [...prev, {
              role: 'assistant',
              content: `Indexed **${repoName}** — ${stats.files_indexed} files, ${stats.chunks_total} chunks added to semantic memory.`,
              files: [],
            }])
          }}
        />
      )}
    </div>
  )
}

export default App
