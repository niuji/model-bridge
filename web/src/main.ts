import { createApp } from 'vue'
import { createRouter, createWebHashHistory } from 'vue-router'
import App from './App.vue'

const routes = [
  { path: '/', component: () => import('./views/Dashboard.vue') },
  { path: '/models', component: () => import('./views/Models.vue') },
  { path: '/providers', component: () => import('./views/Providers.vue') },
  { path: '/api-keys', component: () => import('./views/ApiKeys.vue') },
  { path: '/logs', component: () => import('./views/Logs.vue') },
  { path: '/help', component: () => import('./views/Help.vue') },
]

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    ...routes,
    ...routes.map(route => ({
      ...route,
      path: `/system-one${route.path === '/' ? '' : route.path}`,
      props: { systemOne: true },
      meta: { systemOne: true },
    })),
  ],
})

const app = createApp(App)
app.use(router)
app.mount('#app')
