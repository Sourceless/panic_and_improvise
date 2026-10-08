Feature: Sliding

  Background:
    Given I am in test room 1

  Scenario: Crouching in to a sprint slides, fast at first and then slowing to a crouch
    Given I hold W
    And I hold Shift
    And I am moving faster than 9 metres per second
    When I press C
    Then I am crouching
    And I am moving faster than 9 metres per second
    When I release Shift
    Then I am moving slower than 4 metres per second
    And I am crouching

  Scenario: Crouching while only walking is just a crouch
    Given I hold W
    And I wait 0.5 seconds
    When I press C
    Then I am crouching
    And I am moving slower than 4 metres per second

  Scenario: Jumping ends a slide
    Given I hold W
    And I hold Shift
    And I am moving faster than 9 metres per second
    And I press C
    When I press Space
    Then I am standing
