'use client'

import { useParams } from 'next/navigation'

import { getDocsDictionary } from '../../lib/docsDictionary.mjs'

export default function NotFoundPage() {
  const { lang } = useParams()
  const { notFound } = getDocsDictionary(lang)

  return (
    <>
      <h1>{notFound.title}</h1>
      <p>{notFound.description}</p>
      <a className="kv-button kv-button-primary" href={`/${lang}`}>{notFound.homeLink}</a>
    </>
  )
}
