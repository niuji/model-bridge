import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

import Dashboard from './views/Dashboard.vue'
import Providers from './views/Providers.vue'
import ApiKeys from './views/ApiKeys.vue'
import Logs from './views/Logs.vue'

const routes = [
  { path: '/', component: Dashboard },
  { path: '/providers', component: Providers },
  { path: '/api-keys', component: ApiKeys },
  { path: '/logs', component: Logs },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

const app = createApp(App)
app.use(router)
app.mount('#app')
