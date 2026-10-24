import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './styles/base.css'
import './styles/toolbar.css'
import './styles/popovers.css'
import './styles/legend.css'
import './styles/tooltips.css'
import './styles/panels.css'
import './styles/phone.css'
import './styles/inspect.css'
import './styles/debug.css'
import './styles/pills.css'
import './styles/cards.css'
import './styles/construction.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
