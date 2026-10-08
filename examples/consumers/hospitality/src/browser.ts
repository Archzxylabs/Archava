import { mount, type ChatHostLike } from '@archava/chat'
import { ask, config } from './main.js'

const element = document.querySelector<HTMLElement>('#archava')
if (!element) throw new Error('Missing Archava mount')
const chat = mount(element as unknown as ChatHostLike, { branding: config.branding })
chat.show(await ask())
