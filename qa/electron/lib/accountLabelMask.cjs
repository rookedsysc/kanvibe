/**
 * 문서용 캡처에서 실제 계정 이메일을 예시 주소로 바꾼다.
 *
 * 퍼센트·등급·초기화 시각 같은 조회 결과는 손대지 않는다. 화면 전체를 꾸며 놓으면
 * 그 컷이 증명해야 할 "진짜 구독 상태가 보인다"가 무너진다.
 *
 * 가릴 라벨을 하나도 못 찾으면 실패시킨다. 조용히 넘어가면 실제 이메일이 그대로 찍힌
 * 이미지가 문서에 커밋되는데, 그건 캡처가 끝난 뒤에는 되돌릴 수 없는 종류의 실수다.
 */
const MASKED_EMAIL = "you@example.com";

async function maskAccountLabels(page, testIds) {
  const maskedLabelCount = await page.evaluate(({ testIds, maskedEmail }) => {
    const selector = testIds.map((testId) => `[data-testid='${testId}']`).join(", ");
    const labels = [...document.querySelectorAll(selector)];
    for (const label of labels) {
      label.textContent = maskedEmail;
      label.setAttribute("title", maskedEmail);
    }
    return labels.length;
  }, { testIds, maskedEmail: MASKED_EMAIL });

  if (maskedLabelCount === 0) {
    throw new Error(`가릴 계정 라벨을 찾지 못했습니다: ${testIds.join(", ")}`);
  }

  return maskedLabelCount;
}

module.exports = { maskAccountLabels, MASKED_EMAIL };
