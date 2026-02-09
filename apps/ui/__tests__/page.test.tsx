import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: vi.fn(),
  }),
  useSearchParams: () => new URLSearchParams(),
}))

vi.mock('@/lib/api', () => ({
  api: {
    listRuns: vi.fn().mockResolvedValue({ runs: [], total: 0 }),
  },
}))

import Page from '../src/app/page'

describe('Home Page', () => {
  it('renders the MLRunX heading', async () => {
    render(<Page />)
    const heading = screen.getByRole('heading', { level: 1 })
    expect(heading).toBeDefined()
<<<<<<< HEAD
    expect(heading.textContent).toContain('MLRunX')
    const emptyStates = await screen.findAllByText(/no runs found/i)
    expect(emptyStates.length).toBeGreaterThan(0)
=======
    expect(heading.textContent).toContain('Experiments')
    await screen.findByText(/no runs found/i)
>>>>>>> 6c42f93 (Fix API sqlite runtime init and align integration/UI tests)
  })

  it('displays the welcome message', async () => {
    render(<Page />)
    const text = screen.getByText(/track, compare and analyze your ml training runs/i)
    expect(text).toBeDefined()
    const emptyStates = await screen.findAllByText(/no runs found/i)
    expect(emptyStates.length).toBeGreaterThan(0)
  })
})
