import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

const routes = [
  { path: '/', component: () => import('./views/Dashboard.vue') },
  { path: '/providers', component: () => import('./views/Providers.vue') },
  { path: '/api-keys', component: () => import('./views/ApiKeys.vue') },
  { path: '/logs', component: () => import('./views/Logs.vue') },
  { path: '/help', component: () => import('./views/Help.vue') },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

const app = createApp(App)
app.use(router)
app.mount('#app')
